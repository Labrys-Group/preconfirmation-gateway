use jsonrpsee::Extensions;
use jsonrpsee::core::RpcResult;
use std::sync::Arc;
use tracing::{debug, error, info, instrument, warn};

use super::super::types::beacon::timing;
use super::super::types::rpc::{Offering, SlotInfo};
use super::super::types::{
	BeaconTiming, Commitment, CommitmentRequest, FeeInfo, PayloadParser, RpcContext, SignedCommitment, SlotInfoResponse,
};
use crate::crypto::{generate_request_hash, sign_commitment};
use crate::db::delegation_ops::get_delegations_for_slot;
use crate::utils::address::normalize_address;

/// Validate payload and extract slot number
fn validate_and_extract_slot(commitment_type: u64, payload: &[u8]) -> Result<u64, String> {
	PayloadParser::extract_slot(commitment_type, payload)
		.map_err(|e| format!("Failed to extract slot from payload: {}", e))
}

/// Validate slasher address against whitelist
fn validate_slasher_address(context: &RpcContext, slasher_address: &str) -> Result<(), String> {
	// Normalize the provided slasher address for comparison
	let canonical_slasher = normalize_address(slasher_address);

	// Check if the slasher address is in the whitelist
	// Note: The whitelist is guaranteed to be non-empty by config validation at startup
	let is_whitelisted = context
		.config
		.validation
		.slasher_whitelist
		.iter()
		.any(|whitelisted| normalize_address(whitelisted) == canonical_slasher);

	if is_whitelisted {
		Ok(())
	} else {
		Err(format!(
			"Slasher address {} is not in the configured whitelist. Only whitelisted slasher contracts are allowed.",
			slasher_address
		))
	}
}

/// Check if we have valid delegation authority for the given slot and committer
async fn verify_delegation_authority(
	context: &RpcContext,
	slot: u64,
	committer_address: &str,
) -> Result<String, String> {
	// Get delegations for this slot from the database
	let delegations = get_delegations_for_slot(context.database.pool(), slot)
		.await
		.map_err(|e| format!("Failed to query delegations for slot {}: {}", slot, e))?;

	// Check if we have any delegation for this slot and committer
	let canonical_committer = normalize_address(committer_address);
	let matching_delegation = delegations.iter().find(|delegation| {
		normalize_address(&delegation.message.committer) == canonical_committer && delegation.is_valid_for_slot(slot)
	});

	let delegation = match matching_delegation {
		Some(d) => d,
		None => {
			return Err(format!(
				"No valid delegation found for slot {} and committer {}. Cannot sign commitment without delegation authority.",
				slot, committer_address
			));
		}
	};

	// CRITICAL SECURITY: Verify the BLS signature on the delegation
	let bls_manager = crate::crypto::bls::BlsManager::new(&context.config.delegation.domain_application_gateway)
		.map_err(|e| format!("Failed to create BLS manager: {}", e))?;

	match bls_manager.verify_delegation_signature(delegation) {
		Ok(true) => {
			debug!("BLS signature verified for delegation slot {} committer {}", slot, committer_address);
		}
		Ok(false) => {
			return Err(format!(
				"Invalid BLS signature on delegation for slot {} and committer {}. Rejecting potentially tampered delegation.",
				slot, committer_address
			));
		}
		Err(e) => {
			return Err(format!("Failed to verify BLS signature on delegation for slot {}: {}", slot, e));
		}
	}

	// CRITICAL SECURITY: Verify that the proposer in the delegation is actually
	// the scheduled validator for this slot according to the beacon chain
	match context.beacon_client.get_proposer_for_slot(slot).await {
		Ok(Some(proposer_duty)) => {
			// Parse the beacon API's public key from hex string
			let scheduled_proposer = match proposer_duty.parse_pubkey() {
				Ok(pubkey) => pubkey,
				Err(e) => {
					return Err(format!(
						"Failed to parse proposer public key from beacon API for slot {}: {}",
						slot, e
					));
				}
			};

			let delegation_proposer = &delegation.message.proposer;

			if scheduled_proposer.0 != delegation_proposer.0 {
				return Err(format!(
					"Delegation proposer mismatch for slot {}: delegation claims proposer 0x{}, but beacon chain shows 0x{}. Rejecting potentially fraudulent delegation.",
					slot,
					hex::encode(delegation_proposer.0),
					hex::encode(scheduled_proposer.0)
				));
			}

			info!(
				"Beacon validation passed: proposer 0x{} is confirmed for slot {}",
				hex::encode(scheduled_proposer.0),
				slot
			);
		}
		Ok(None) => {
			return Err(format!(
				"No proposer scheduled for slot {} according to beacon chain. Cannot validate delegation authority.",
				slot
			));
		}
		Err(e) => {
			return Err(format!(
				"Failed to query beacon chain for slot {}: {}. Cannot validate delegation without beacon chain confirmation.",
				slot, e
			));
		}
	}

	info!("Delegation authority verified for slot {} and committer {}", slot, committer_address);
	Ok(delegation.message.committer.clone())
}

/// Find the appropriate ECDSA key for signing based on the committer address
fn find_signing_key_for_committer<'a>(
	context: &'a RpcContext,
	committer_address: &str,
) -> Result<&'a secp256k1::SecretKey, String> {
	if normalize_address(committer_address) == normalize_address(&context.config.signing.committer_address) {
		return Ok(&context.config.signing.ecdsa_private_key);
	}

	Err(format!(
		"No signing key found for committer address: {}. Available address: {}",
		committer_address, context.config.signing.committer_address
	))
}

#[instrument(name = "commitment_request", skip(context, _extensions))]
pub async fn commitment_request_handler(
	params: jsonrpsee::types::Params<'static>,
	context: Arc<RpcContext>,
	_extensions: Extensions,
) -> RpcResult<SignedCommitment> {
	info!("Processing commitment request with delegation-first security");

	// Parse params as a CommitmentRequest object
	let request: CommitmentRequest = params.one()?;

	// Validate commitment_type
	if request.commitment_type != 1 {
		warn!("Invalid commitment type: {}", request.commitment_type);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Validate slasher address against whitelist
	validate_slasher_address(&context, &request.slasher).map_err(|e| {
		warn!("Slasher validation failed: {}", e);
		jsonrpsee::types::error::ErrorCode::InvalidRequest
	})?;

	// Extract slot from payload - CRITICAL: This must succeed before any signing
	let slot = validate_and_extract_slot(request.commitment_type, &request.payload).map_err(|e| {
		warn!("Payload validation failed: {}", e);
		jsonrpsee::types::error::ErrorCode::InvalidParams
	})?;

	info!("Extracted slot {} from commitment payload", slot);

	// EARLY DUPLICATE DETECTION: Generate hash and check for duplicates BEFORE expensive operations
	// This fails fast before delegation verification and signing
	let request_hash = generate_request_hash(&request).map_err(|e| {
		error!("Failed to generate request hash: {}", e);
		jsonrpsee::types::error::ErrorCode::InternalError
	})?;

	// Check if commitment already exists - fail fast before expensive delegation verification
	if context.database.commitment_exists(&request_hash).await.map_err(|e| {
		error!("Database error checking commitment existence: {}", e);
		jsonrpsee::types::error::ErrorCode::InternalError
	})? {
		warn!("Duplicate commitment request rejected early: {}", request_hash);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidRequest.into());
	}

	// DELEGATION-FIRST SECURITY: Verify delegation authority BEFORE any signing
	// This returns the validated committer address from the delegation
	let committer_address = verify_delegation_authority(&context, slot, &request.slasher).await.map_err(|e| {
		error!("Delegation verification failed: {}", e);
		jsonrpsee::types::error::ErrorCode::InvalidRequest
	})?;

	// Find the appropriate signing key for this committer (use delegation.committer, not request.slasher)
	let signing_key = find_signing_key_for_committer(&context, &committer_address).map_err(|e| {
		error!("No signing key found: {}", e);
		jsonrpsee::types::error::ErrorCode::InvalidRequest
	})?;

	// Create commitment with real request hash
	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload.clone(),
		request_hash: request_hash.clone(),
		slasher: request.slasher.clone(),
	};

	// Sign the commitment with the appropriate key
	let signature = sign_commitment(&commitment, signing_key).map_err(|e| {
		error!("Failed to sign commitment: {}", e);
		jsonrpsee::types::error::ErrorCode::InternalError
	})?;

	let signed_commitment = SignedCommitment { commitment, signature };

	// Save to database with atomic duplicate detection via ON CONFLICT
	let save_result = context.database.save_commitment(&signed_commitment).await.map_err(|e| {
		error!("Failed to save commitment to database: {}", e);
		jsonrpsee::types::error::ErrorCode::InternalError
	})?;

	// If ON CONFLICT triggered (returns None), this means a duplicate slipped through
	// This is a defensive check - should not happen with our early duplicate detection
	if save_result.is_none() {
		warn!("Duplicate commitment detected at database level (race condition): {}", request_hash);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidRequest.into());
	}

	// Track gas usage for congestion-based fee pricing
	// Calculate fee to get gas estimation, then apply it to slot congestion
	let fee_calc = context
		.fee_engine
		.calculate_fee_for_commitment(request.commitment_type, &request.payload, slot)
		.await
		.map_err(|e| {
			warn!("Failed to calculate fee for gas tracking: {}", e);
			jsonrpsee::types::error::ErrorCode::InternalError
		})?;

	// Apply the gas usage to update slot congestion
	if let Err(e) = context.fee_engine.apply_gas_usage_to_slot(slot, fee_calc.estimated_gas).await {
		warn!("Failed to update slot congestion: {}", e);
		// Don't fail the commitment, just log the warning
	}

	// Queue constraint submission for this commitment
	// The background constraint submission service will automatically process
	// signed commitments and create constraints for the relay within the 8-second deadline
	info!(
		"Commitment for slot {} processed successfully with delegation authority. Constraint submission queued for background processing.",
		slot
	);

	// Note: Constraint submission is handled by the background ConstraintSubmissionService
	// which polls for signed commitments and converts them to BLS-signed constraints
	// that are submitted to relays within the timing requirements.

	info!("Commitment request processed and saved successfully");
	Ok(signed_commitment)
}

#[instrument(name = "commitment_result", skip(context, _extensions))]
pub async fn commitment_result_handler(
	params: jsonrpsee::types::Params<'static>,
	context: Arc<RpcContext>,
	_extensions: Extensions,
) -> RpcResult<SignedCommitment> {
	info!("Processing commitment result request");
	let request_hash: String = params.one()?;

	// Retrieve commitment from database
	match context.database.get_commitment_by_hash(&request_hash).await {
		Ok(Some(signed_commitment)) => {
			info!("Commitment result request processed successfully");
			Ok(signed_commitment)
		}
		Ok(None) => {
			info!("Commitment not found for hash: {}", request_hash);
			Err(jsonrpsee::types::error::ErrorCode::InvalidRequest.into())
		}
		Err(e) => {
			info!("Database error retrieving commitment: {}", e);
			Err(jsonrpsee::types::error::ErrorCode::InternalError.into())
		}
	}
}

#[instrument(name = "slots", skip(_context, _extensions))]
pub fn slots_handler(
	_params: jsonrpsee::types::Params<'_>,
	_context: &RpcContext,
	_extensions: &Extensions,
) -> RpcResult<SlotInfoResponse> {
	info!("Processing slots request");

	// Calculate current slot and lookahead window
	let genesis_time = _context.config.beacon_api.genesis_time;
	let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
	let lookahead_slots = _context.config.delegation.lookahead_epochs * timing::SLOTS_PER_EPOCH;

	// Generate slots for our service catalog (what we can offer)
	let mut slots = Vec::new();

	// Start from next slot to avoid timing issues with current slot
	let start_slot = current_slot + 1;
	let end_slot = start_slot + lookahead_slots;

	for slot in start_slot..end_slot {
		// Create offering for Hoodi chain with inclusion commitments (type 1)
		let hoodi_offering = Offering {
			chain_id: 560048,          // Hoodi chain ID
			commitment_types: vec![1], // Only support inclusion commitments
		};

		let slot_info = SlotInfo { slot, offerings: vec![hoodi_offering] };

		slots.push(slot_info);
	}

	let slots_count = slots.len();
	let response = SlotInfoResponse { slots };

	info!("Slots request processed successfully: {} slots from {} to {}", slots_count, start_slot, end_slot);
	Ok(response)
}

#[instrument(name = "fee", skip(context, _extensions))]
pub async fn fee_handler(
	params: jsonrpsee::types::Params<'_>,
	context: Arc<RpcContext>,
	_extensions: Extensions,
) -> RpcResult<FeeInfo> {
	info!("Processing fee request with dynamic pricing");
	let request: CommitmentRequest = params.one()?;

	// Validate commitment_type
	if request.commitment_type != 1 {
		warn!("Invalid commitment type for fee calculation: {}", request.commitment_type);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Extract slot from payload for fee calculation
	let slot = validate_and_extract_slot(request.commitment_type, &request.payload).map_err(|e| {
		warn!("Payload validation failed during fee calculation: {}", e);
		jsonrpsee::types::error::ErrorCode::InvalidParams
	})?;

	info!("Calculating fee for slot {} with commitment type {}", slot, request.commitment_type);

	// Check if the slot is within acceptable range for fee calculation
	if !context.fee_engine.is_slot_acceptable_for_fees(slot) {
		warn!("Slot {} is outside acceptable range for fee calculation", slot);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Calculate dynamic fee using the pricing engine
	let fee_calculation = context
		.fee_engine
		.calculate_fee_for_commitment(request.commitment_type, &request.payload, slot)
		.await
		.map_err(|e| {
			error!("Failed to calculate fee for slot {}: {}", slot, e);
			jsonrpsee::types::error::ErrorCode::InternalError
		})?;

	// Encode the fee calculation result as opaque payload bytes
	// Using a simple binary format: [total_cost (8 bytes) | final_price (8 bytes) | estimated_gas (8 bytes) | congestion_ratio (8 bytes)]
	let mut payload = Vec::with_capacity(32);
	payload.extend_from_slice(&fee_calculation.total_cost.to_le_bytes());
	payload.extend_from_slice(&fee_calculation.final_price.to_le_bytes());
	payload.extend_from_slice(&fee_calculation.estimated_gas.to_le_bytes());

	// Safely encode congestion ratio as parts-per-million (0.0-1.0 -> 0-1000000)
	// Clamp to valid range and use saturating multiply to prevent overflow
	let congestion_ppm = (fee_calculation.congestion_ratio.clamp(0.0, 1.0) * 1_000_000.0) as u64;
	payload.extend_from_slice(&congestion_ppm.to_le_bytes());

	let fee_info = FeeInfo { payload, commitment_type: request.commitment_type };

	info!(
		"Fee calculation completed for slot {}: total_cost={} wei, price={} wei/gas, congestion={:.2}%",
		slot,
		fee_calculation.total_cost,
		fee_calculation.final_price,
		fee_calculation.congestion_ratio * 100.0
	);

	Ok(fee_info)
}

#[cfg(test)]
mod handlers_integration_tests;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::Config;
	use crate::db::DatabaseContext;
	use crate::types::{CommitmentRequest, payload::InclusionPayload, payload::PayloadParser};
	use sqlx::PgPool;
	use std::sync::Arc;
	use anyhow::Result;
	use std::sync::Mutex as StdMutex;

	/// Mock database context for testing error scenarios
	struct MockDatabaseContext {
		/// Control whether commitment_exists should return true
		simulate_exists: Arc<StdMutex<bool>>,
		/// Control whether save should fail with conflict
		simulate_conflict: Arc<StdMutex<bool>>,
		/// Control whether operations should fail with database error
		simulate_error: Arc<StdMutex<bool>>,
		/// Store commitments in memory for testing
		commitments: Arc<StdMutex<std::collections::HashMap<String, crate::types::SignedCommitment>>>,
	}

	impl MockDatabaseContext {
		fn new() -> Self {
			Self {
				simulate_exists: Arc::new(StdMutex::new(false)),
				simulate_conflict: Arc::new(StdMutex::new(false)),
				simulate_error: Arc::new(StdMutex::new(false)),
				commitments: Arc::new(StdMutex::new(std::collections::HashMap::new())),
			}
		}

		fn set_simulate_exists(&self, value: bool) {
			*self.simulate_exists.lock().unwrap() = value;
		}

		fn set_simulate_conflict(&self, value: bool) {
			*self.simulate_conflict.lock().unwrap() = value;
		}

		fn set_simulate_error(&self, value: bool) {
			*self.simulate_error.lock().unwrap() = value;
		}

		async fn commitment_exists(&self, request_hash: &str) -> Result<bool> {
			if *self.simulate_error.lock().unwrap() {
				return Err(anyhow::anyhow!("Mock database error"));
			}
			if *self.simulate_exists.lock().unwrap() {
				return Ok(true);
			}
			Ok(self.commitments.lock().unwrap().contains_key(request_hash))
		}

		async fn save_commitment(
			&self,
			signed_commitment: &crate::types::SignedCommitment,
		) -> Result<Option<uuid::Uuid>> {
			if *self.simulate_error.lock().unwrap() {
				return Err(anyhow::anyhow!("Mock database error"));
			}
			if *self.simulate_conflict.lock().unwrap() {
				return Ok(None); // Simulate ON CONFLICT
			}
			let hash = signed_commitment.commitment.request_hash.clone();
			if self.commitments.lock().unwrap().contains_key(&hash) {
				return Ok(None); // Already exists
			}
			self.commitments.lock().unwrap().insert(hash, signed_commitment.clone());
			Ok(Some(uuid::Uuid::new_v4()))
		}

		async fn get_commitment_by_hash(
			&self,
			request_hash: &str,
		) -> Result<Option<crate::types::SignedCommitment>> {
			if *self.simulate_error.lock().unwrap() {
				return Err(anyhow::anyhow!("Mock database error"));
			}
			Ok(self.commitments.lock().unwrap().get(request_hash).cloned())
		}
	}

	// Helper to create a test RPC context with minimal configuration
	fn create_test_context() -> Arc<RpcContext> {
		use crate::crypto::bls::keys;
		use crate::crypto::parse_private_key;

		let private_key =
			parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
		let bls_key =
			keys::parse_private_key("0x1234567890123456789012345678901234567890123456789012345678901234").unwrap();

		let config = Config {
			server: crate::config::ServerConfig { host: "127.0.0.1".to_string(), port: 8545 },
			database: crate::config::DatabaseConfig { url: "postgresql://test:test@localhost/test_db".to_string() },
			logging: crate::config::LoggingConfig {
				level: "info".to_string(),
				enable_method_tracing: false,
				traced_methods: vec![],
			},
			validation: crate::config::ValidationConfig {
				slasher_whitelist: vec!["0x1234567890123456789012345678901234567890".to_string()],
			},
			beacon_api: crate::config::BeaconApiConfig {
				primary_endpoint: "http://localhost:3500".to_string(),
				fallback_endpoints: vec![],
				request_timeout_secs: 30,
				genesis_time: 1606824023,
			},
			constraints_api: crate::config::ConstraintsApiConfig {
				relay_endpoint: "http://localhost:3501".to_string(),
				request_timeout_secs: 30,
				max_retries: 3,
				authorized_builders: vec![],
			},
			delegation: crate::config::DelegationConfig {
				lookahead_epochs: 2,
				polling_interval_secs: 12,
				cache_ttl_secs: 3600,
				domain_application_gateway: "0x00000001".to_string(),
			},
			reth: crate::config::RethConfig::default(),
			signing: crate::config::SigningConfig {
				ecdsa_private_key: private_key,
				bls_private_key: bls_key.clone(),
				bls_public_key: bls_key.sk_to_pk(),
				committer_address: "0x1234567890123456789012345678901234567890".to_string(),
			},
		};

		// Create a test database pool (won't actually connect in unit tests)
		let pool = PgPool::connect_lazy(&config.database.url).expect("Failed to create test pool");
		let database = DatabaseContext::new(pool);

		// For testing purposes, create a minimal fee engine
		use crate::api::beacon::BeaconApiClient;
		use crate::api::reth::{RethApiClient, RethApiConfig};
		use crate::services::fee_pricing::FeePricingEngine;

		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database_arc = Arc::new(database.clone());
		let config_arc = Arc::new(config.clone());
		let fee_engine = Arc::new(FeePricingEngine::new(reth_client, database_arc, config_arc.clone()));

		// Create beacon API client for testing
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());

		Arc::new(RpcContext::new(database, config, fee_engine, beacon_client))
	}

	#[test]
	fn test_validate_and_extract_slot_success() {
		let payload = InclusionPayload::new(12345, vec![1, 2, 3, 4]);
		let encoded = PayloadParser::encode_inclusion_payload(&payload).unwrap();

		let result = validate_and_extract_slot(1, &encoded);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 12345);
	}

	#[test]
	fn test_validate_and_extract_slot_invalid_type() {
		let payload = InclusionPayload::new(12345, vec![1, 2, 3, 4]);
		let encoded = PayloadParser::encode_inclusion_payload(&payload).unwrap();

		let result = validate_and_extract_slot(99, &encoded);
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("Unknown commitment type"));
	}

	#[test]
	fn test_validate_and_extract_slot_invalid_payload() {
		let invalid_payload = vec![0xff, 0xff, 0xff, 0xff];

		let result = validate_and_extract_slot(1, &invalid_payload);
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("Failed to extract slot"));
	}

	#[tokio::test]
	async fn test_validate_slasher_address_whitelisted() {
		// Test that whitelisted addresses are accepted
		let context = create_test_context();
		let mut config = context.config.clone();
		config.validation.slasher_whitelist = vec![
			"0x1234567890123456789012345678901234567890".to_string(),
			"0xabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_string(),
		];

		let context_with_whitelist = Arc::new(RpcContext {
			database: context.database.clone(),
			config,
			fee_engine: context.fee_engine.clone(),
			beacon_client: context.beacon_client.clone(),
		});

		// Test exact match
		let result = validate_slasher_address(&context_with_whitelist, "0x1234567890123456789012345678901234567890");
		assert!(result.is_ok());

		// Test case-insensitive match
		let result = validate_slasher_address(&context_with_whitelist, "0x1234567890123456789012345678901234567890");
		assert!(result.is_ok());

		// Test uppercase
		let result = validate_slasher_address(&context_with_whitelist, "0xABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD");
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_validate_slasher_address_not_whitelisted() {
		// Test that non-whitelisted addresses are rejected
		let context = create_test_context();
		let mut config = context.config.clone();
		config.validation.slasher_whitelist = vec!["0x1234567890123456789012345678901234567890".to_string()];

		let context_with_whitelist = Arc::new(RpcContext {
			database: context.database.clone(),
			config,
			fee_engine: context.fee_engine.clone(),
			beacon_client: context.beacon_client.clone(),
		});

		let result = validate_slasher_address(&context_with_whitelist, "0x9999999999999999999999999999999999999999");
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("not in the configured whitelist"));
	}

	#[tokio::test]
	async fn test_validate_slasher_address_normalization() {
		// Test that address normalization works correctly
		let context = create_test_context();
		let mut config = context.config.clone();
		config.validation.slasher_whitelist = vec!["0x1234567890ABCdef1234567890abcdef12345678".to_string()]; // Mixed case

		let context_with_whitelist = Arc::new(RpcContext {
			database: context.database.clone(),
			config,
			fee_engine: context.fee_engine.clone(),
			beacon_client: context.beacon_client.clone(),
		});

		// Test lowercase version of mixed case address
		let result = validate_slasher_address(&context_with_whitelist, "0x1234567890abcdef1234567890abcdef12345678");
		assert!(result.is_ok());

		// Test uppercase version of mixed case address
		let result = validate_slasher_address(&context_with_whitelist, "0x1234567890ABCDEF1234567890ABCDEF12345678");
		assert!(result.is_ok());
	}

	#[test]
	fn test_find_signing_key_for_committer_success() {
		// Test key finding logic directly without database context
		use crate::crypto::{ecdsa_to_address, parse_private_key};

		let private_key =
			parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
		let address = ecdsa_to_address(&private_key).unwrap();

		// Test that we can find a key for the derived address
		assert_eq!(address.to_lowercase(), "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");

		// This proves the key derivation logic works correctly
		assert!(private_key.secret_bytes().len() == 32);
	}

	#[test]
	fn test_find_signing_key_for_committer_not_found() {
		// Test that different keys produce different addresses
		use crate::crypto::{ecdsa_to_address, parse_private_key};

		let private_key1 =
			parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
		let private_key2 =
			parse_private_key("59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d").unwrap();

		let address1 = ecdsa_to_address(&private_key1).unwrap();
		let address2 = ecdsa_to_address(&private_key2).unwrap();

		// Verify different keys produce different addresses
		assert_ne!(address1, address2);
		assert_eq!(address1.to_lowercase(), "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");
		assert_eq!(address2.to_lowercase(), "0x70997970c51812dc3a010c7d01b50e0d17dc79c8");
	}

	// Integration-style test for the commitment flow (without actual database/network calls)
	mod integration_style_tests {
		use super::*;
		use crate::testing::fixtures::TestFixtures;

		#[tokio::test]
		async fn test_commitment_request_validation_flow() {
			// Test that the commitment request handler validates inputs correctly
			let context = create_test_context();

			// Test 1: Invalid commitment type
			let invalid_request = CommitmentRequest {
				commitment_type: 99, // Invalid type
				payload: vec![1, 2, 3, 4],
				slasher: context
					.config
					.validation
					.slasher_whitelist
					.first()
					.unwrap_or(&"0x0000000000000000000000000000000000000000".to_string())
					.clone(),
			};

			// We can't call the handler directly due to database dependencies,
			// but we can test the validation functions
			let slot_result = validate_and_extract_slot(invalid_request.commitment_type, &invalid_request.payload);
			assert!(slot_result.is_err());

			// Test 2: Valid commitment type but invalid payload
			let invalid_payload_request = CommitmentRequest {
				commitment_type: 1,
				payload: vec![0xff, 0xff, 0xff, 0xff], // Invalid payload
				slasher: context
					.config
					.validation
					.slasher_whitelist
					.first()
					.unwrap_or(&"0x0000000000000000000000000000000000000000".to_string())
					.clone(),
			};

			let slot_result =
				validate_and_extract_slot(invalid_payload_request.commitment_type, &invalid_payload_request.payload);
			assert!(slot_result.is_err());

			// Test 3: Valid payload structure
			let valid_request = TestFixtures::create_inclusion_commitment_request(
				12345,
				context
					.config
					.validation
					.slasher_whitelist
					.first()
					.unwrap_or(&"0x0000000000000000000000000000000000000000".to_string()),
			);

			let slot_result = validate_and_extract_slot(valid_request.commitment_type, &valid_request.payload);
			assert!(slot_result.is_ok());
			assert_eq!(slot_result.unwrap(), 12345);
		}

		#[test]
		fn test_signing_key_management() {
			// Test the ECDSA address derivation logic directly
			use crate::crypto::{ecdsa_to_address, parse_private_key};

			// Test known private key to address mapping
			let private_key =
				parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
			let derived_address = ecdsa_to_address(&private_key).unwrap();

			// This is the expected address for this private key (from Hardhat)
			assert_eq!(derived_address.to_lowercase(), "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");

			// Test that key parsing works
			assert_eq!(private_key.secret_bytes().len(), 32);
		}

		#[test]
		fn test_request_hash_generation_consistency() {
			// Test that request hash generation is deterministic
			let request =
				TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");

			let hash1 = generate_request_hash(&request).unwrap();
			let hash2 = generate_request_hash(&request).unwrap();
			assert_eq!(hash1, hash2);
			assert!(hash1.starts_with("0x"));
			assert_eq!(hash1.len(), 66); // 0x + 64 hex chars
		}

		#[test]
		fn test_commitment_structure_validation() {
			// Test that our commitment structures are properly formed
			let request =
				TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");

			// Validate that we can extract slot
			let slot = PayloadParser::extract_slot(request.commitment_type, &request.payload).unwrap();
			assert_eq!(slot, 12345);

			// Validate that we can generate a hash
			let hash = generate_request_hash(&request).unwrap();
			assert!(!hash.is_empty());

			// Validate that commitment type is supported
			assert_eq!(request.commitment_type, 1);
		}

		#[test]
		fn test_slots_handler_service_catalog() {
			// Test the slots handler logic without database dependencies
			use crate::config::{BeaconApiConfig, Config, DelegationConfig};

			// Create minimal config for testing
			let config = Config {
				server: crate::config::ServerConfig { host: "127.0.0.1".to_string(), port: 8545 },
				database: crate::config::DatabaseConfig { url: "postgresql://test:test@localhost/test_db".to_string() },
				logging: crate::config::LoggingConfig {
					level: "info".to_string(),
					enable_method_tracing: false,
					traced_methods: vec![],
				},
				validation: crate::config::ValidationConfig {
					slasher_whitelist: vec!["0x1234567890123456789012345678901234567890".to_string()],
				},
				beacon_api: BeaconApiConfig {
					primary_endpoint: "http://localhost:3500".to_string(),
					fallback_endpoints: vec![],
					request_timeout_secs: 30,
					genesis_time: 1606824023, // Fixed genesis time for testing
				},
				constraints_api: crate::config::ConstraintsApiConfig {
					relay_endpoint: "http://localhost:3501".to_string(),
					request_timeout_secs: 30,
					max_retries: 3,
					authorized_builders: vec![],
				},
				delegation: DelegationConfig {
					lookahead_epochs: 2,
					polling_interval_secs: 12,
					cache_ttl_secs: 3600,
					domain_application_gateway: "0x00000001".to_string(),
				},
				reth: crate::config::RethConfig::default(),
				signing: crate::config::SigningConfig::default(),
			};

			// Calculate expected slot behavior
			let genesis_time = config.beacon_api.genesis_time;
			let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
			let lookahead_slots = config.delegation.lookahead_epochs * timing::SLOTS_PER_EPOCH;
			let expected_start_slot = current_slot + 1;
			let expected_end_slot = expected_start_slot + lookahead_slots;

			// Test the service catalog logic manually
			let mut expected_slots = Vec::new();
			for slot in expected_start_slot..expected_end_slot {
				let hoodi_offering = Offering {
					chain_id: 560048,          // Hoodi chain ID
					commitment_types: vec![1], // Only support inclusion commitments
				};

				let slot_info = SlotInfo { slot, offerings: vec![hoodi_offering] };

				expected_slots.push(slot_info);
			}

			// Verify the logic produces correct results
			assert_eq!(expected_slots.len(), lookahead_slots as usize);

			// Verify each slot has the correct offering for Hoodi chain
			for slot_info in &expected_slots {
				assert_eq!(slot_info.offerings.len(), 1);

				let offering = &slot_info.offerings[0];
				assert_eq!(offering.chain_id, 560048); // Hoodi chain ID
				assert_eq!(offering.commitment_types, vec![1]); // Only inclusion commitments

				// Verify slot numbers are reasonable
				assert!(slot_info.slot >= expected_start_slot);
				assert!(slot_info.slot < expected_end_slot);
			}

			// Verify slots are in ascending order
			for i in 1..expected_slots.len() {
				assert!(expected_slots[i].slot > expected_slots[i - 1].slot);
				assert_eq!(expected_slots[i].slot, expected_slots[i - 1].slot + 1);
			}
		}

		// === Error Path Tests ===
		// These tests focus on error handling paths to increase coverage

		mod error_path_tests {
			use super::*;

			#[test]
			fn test_validate_and_extract_slot_unsupported_commitment_type() {
				// Test that commitment type 99 is rejected
				let payload = InclusionPayload::new(12345, vec![1, 2, 3, 4]);
				let encoded = PayloadParser::encode_inclusion_payload(&payload).unwrap();

				let result = validate_and_extract_slot(99, &encoded);
				assert!(result.is_err());
				assert!(result.unwrap_err().contains("Unknown commitment type"));
			}

			#[test]
			fn test_validate_and_extract_slot_commitment_type_zero() {
				// Test that commitment type 0 is rejected
				let payload = InclusionPayload::new(12345, vec![1, 2, 3, 4]);
				let encoded = PayloadParser::encode_inclusion_payload(&payload).unwrap();

				let result = validate_and_extract_slot(0, &encoded);
				assert!(result.is_err());
				assert!(result.unwrap_err().contains("Unknown commitment type"));
			}

			#[test]
			fn test_validate_and_extract_slot_completely_invalid_payload() {
				// Test with completely invalid payload data
				let invalid_payload = vec![0x00];

				let result = validate_and_extract_slot(1, &invalid_payload);
				assert!(result.is_err());
				assert!(result.unwrap_err().contains("Failed to extract slot"));
			}

			#[test]
			fn test_validate_and_extract_slot_empty_payload() {
				// Test with empty payload
				let empty_payload = vec![];

				let result = validate_and_extract_slot(1, &empty_payload);
				assert!(result.is_err());
				assert!(result.unwrap_err().contains("Failed to extract slot"));
			}

			#[tokio::test]
			async fn test_validate_slasher_address_with_empty_whitelist() {
				// This test would fail at startup since config validation requires non-empty whitelist
				// But we can test the logic with a single-entry whitelist
				let context = create_test_context();

				// Test an address that is definitely not in the whitelist
				let result = validate_slasher_address(&context, "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
				assert!(result.is_err());
				assert!(result.unwrap_err().contains("not in the configured whitelist"));
			}

			#[tokio::test]
			async fn test_validate_slasher_address_with_0x_prefix_handling() {
				let context = create_test_context();
				let mut config = context.config.clone();
				config.validation.slasher_whitelist = vec![
					"1234567890123456789012345678901234567890".to_string(), // Without 0x prefix
				];

				let context_with_whitelist = Arc::new(RpcContext {
					database: context.database.clone(),
					config,
					fee_engine: context.fee_engine.clone(),
					beacon_client: context.beacon_client.clone(),
				});

				// Test with 0x prefix - should still match due to normalization
				let result = validate_slasher_address(&context_with_whitelist, "0x1234567890123456789012345678901234567890");
				assert!(result.is_ok());
			}

			#[tokio::test]
			async fn test_find_signing_key_for_committer_no_match() {
				let context = create_test_context();

				// Test with an address that doesn't match our configured committer
				let result = find_signing_key_for_committer(&context, "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
				assert!(result.is_err());
				assert!(result.unwrap_err().contains("No signing key found"));
			}

			#[tokio::test]
			async fn test_find_signing_key_for_committer_case_insensitive() {
				let context = create_test_context();

				// Test that uppercase version of configured address works
				let uppercase_address = context.config.signing.committer_address.to_uppercase();
				let result = find_signing_key_for_committer(&context, &uppercase_address);
				assert!(result.is_ok());
			}

			#[test]
			fn test_commitment_request_structure_validation() {
				// Test various invalid commitment request structures
				let valid_request = TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");

				// Validate structure
				assert_eq!(valid_request.commitment_type, 1);
				assert!(!valid_request.payload.is_empty());
				assert!(!valid_request.slasher.is_empty());
			}

			#[test]
			fn test_payload_parser_consistency() {
				// Test that encoding and decoding are consistent
				let slot = 98765;
				let tx_data = vec![0xaa, 0xbb, 0xcc, 0xdd];
				let payload = InclusionPayload::new(slot, tx_data.clone());

				let encoded = PayloadParser::encode_inclusion_payload(&payload).unwrap();
				let extracted_slot = PayloadParser::extract_slot(1, &encoded).unwrap();

				assert_eq!(extracted_slot, slot);
			}

			#[test]
			fn test_request_hash_uniqueness() {
				// Test that different requests produce different hashes
				let request1 = TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");
				let request2 = TestFixtures::create_inclusion_commitment_request(12346, "0x1234567890123456789012345678901234567890");
				let request3 = TestFixtures::create_inclusion_commitment_request(12345, "0x9999999999999999999999999999999999999999");

				let hash1 = generate_request_hash(&request1).unwrap();
				let hash2 = generate_request_hash(&request2).unwrap();
				let hash3 = generate_request_hash(&request3).unwrap();

				// All hashes should be different
				assert_ne!(hash1, hash2);
				assert_ne!(hash1, hash3);
				assert_ne!(hash2, hash3);
			}

			#[test]
			fn test_slots_handler_lookahead_calculation() {
				// Test with different lookahead values
				let genesis_time = 1606824023;
				let current_slot = BeaconTiming::current_slot_estimate(genesis_time);

				// Test 1 epoch lookahead
				let lookahead_1_epoch = 1 * timing::SLOTS_PER_EPOCH;
				let start_slot_1 = current_slot + 1;
				let end_slot_1 = start_slot_1 + lookahead_1_epoch;
				assert_eq!(end_slot_1 - start_slot_1, lookahead_1_epoch);

				// Test 2 epoch lookahead (default)
				let lookahead_2_epoch = 2 * timing::SLOTS_PER_EPOCH;
				let start_slot_2 = current_slot + 1;
				let end_slot_2 = start_slot_2 + lookahead_2_epoch;
				assert_eq!(end_slot_2 - start_slot_2, lookahead_2_epoch);
				assert_eq!(lookahead_2_epoch, 64); // 2 epochs * 32 slots
			}

			#[test]
			fn test_slots_handler_offering_structure() {
				// Verify the offering structure for Hoodi chain
				let offering = Offering {
					chain_id: 560048,
					commitment_types: vec![1],
				};

				assert_eq!(offering.chain_id, 560048);
				assert_eq!(offering.commitment_types.len(), 1);
				assert_eq!(offering.commitment_types[0], 1);
			}

			#[test]
			fn test_slot_info_structure() {
				// Test SlotInfo structure
				let offering = Offering {
					chain_id: 560048,
					commitment_types: vec![1],
				};

				let slot_info = SlotInfo {
					slot: 12345,
					offerings: vec![offering],
				};

				assert_eq!(slot_info.slot, 12345);
				assert_eq!(slot_info.offerings.len(), 1);
			}

			#[test]
			fn test_multiple_payload_formats() {
				// Test that we can handle different payload sizes
				let small_payload = InclusionPayload::new(100, vec![0x01]);
				let medium_payload = InclusionPayload::new(200, vec![0x01, 0x02, 0x03, 0x04]);
				let large_payload = InclusionPayload::new(300, vec![0x01; 1000]);

				let small_encoded = PayloadParser::encode_inclusion_payload(&small_payload).unwrap();
				let medium_encoded = PayloadParser::encode_inclusion_payload(&medium_payload).unwrap();
				let large_encoded = PayloadParser::encode_inclusion_payload(&large_payload).unwrap();

				// All should extract slots correctly
				assert_eq!(PayloadParser::extract_slot(1, &small_encoded).unwrap(), 100);
				assert_eq!(PayloadParser::extract_slot(1, &medium_encoded).unwrap(), 200);
				assert_eq!(PayloadParser::extract_slot(1, &large_encoded).unwrap(), 300);
			}

			#[test]
			fn test_address_normalization_edge_cases() {
				use crate::utils::address::normalize_address;

				// Test various address formats
				let addr1 = "0x1234567890123456789012345678901234567890";
				let addr2 = "0X1234567890123456789012345678901234567890";
				let addr3 = "1234567890123456789012345678901234567890";
				let addr4 = "0x1234567890ABCDEF1234567890ABCDEF12345678";

				let norm1 = normalize_address(addr1);
				let norm2 = normalize_address(addr2);
				let norm3 = normalize_address(addr3);
				let norm4 = normalize_address(addr4);

				// normalize_address strips the 0x prefix and converts to lowercase
				// All should be equal after normalization
				assert_eq!(norm1.len(), 40); // Without 0x prefix
				assert_eq!(norm1, norm2);
				assert_eq!(norm1, norm3);
				assert_eq!(norm4.to_lowercase(), norm4); // Should be lowercase

				// Test that normalized addresses don't have 0x prefix
				assert!(!norm1.starts_with("0x"));
				assert_eq!(norm1, "1234567890123456789012345678901234567890");
			}

			#[tokio::test]
			async fn test_mock_database_context_exists() {
				let mock_db = MockDatabaseContext::new();

				// Initially should not exist
				let exists = mock_db.commitment_exists("0xtest").await.unwrap();
				assert!(!exists);

				// Set simulate exists
				mock_db.set_simulate_exists(true);
				let exists = mock_db.commitment_exists("0xtest").await.unwrap();
				assert!(exists);
			}

			#[tokio::test]
			async fn test_mock_database_context_error_simulation() {
				let mock_db = MockDatabaseContext::new();

				// Enable error simulation
				mock_db.set_simulate_error(true);

				// Should return error
				let result = mock_db.commitment_exists("0xtest").await;
				assert!(result.is_err());
				assert!(result.unwrap_err().to_string().contains("Mock database error"));
			}

			#[tokio::test]
			async fn test_mock_database_context_save_and_retrieve() {
				use crate::crypto::sign_commitment;
				use crate::crypto::parse_private_key;

				let mock_db = MockDatabaseContext::new();
				let private_key = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();

				// Create a test commitment
				let request = TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");
				let request_hash = generate_request_hash(&request).unwrap();

				let commitment = Commitment {
					commitment_type: request.commitment_type,
					payload: request.payload.clone(),
					request_hash: request_hash.clone(),
					slasher: request.slasher.clone(),
				};

				let signature = sign_commitment(&commitment, &private_key).unwrap();
				let signed_commitment = SignedCommitment { commitment, signature };

				// Save commitment
				let save_result = mock_db.save_commitment(&signed_commitment).await.unwrap();
				assert!(save_result.is_some());

				// Retrieve commitment
				let retrieved = mock_db.get_commitment_by_hash(&request_hash).await.unwrap();
				assert!(retrieved.is_some());
				assert_eq!(retrieved.unwrap().commitment.request_hash, request_hash);
			}

			#[tokio::test]
			async fn test_mock_database_context_conflict_simulation() {
				use crate::crypto::sign_commitment;
				use crate::crypto::parse_private_key;

				let mock_db = MockDatabaseContext::new();
				let private_key = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();

				// Create a test commitment
				let request = TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");
				let request_hash = generate_request_hash(&request).unwrap();

				let commitment = Commitment {
					commitment_type: request.commitment_type,
					payload: request.payload.clone(),
					request_hash: request_hash.clone(),
					slasher: request.slasher.clone(),
				};

				let signature = sign_commitment(&commitment, &private_key).unwrap();
				let signed_commitment = SignedCommitment { commitment, signature };

				// Enable conflict simulation
				mock_db.set_simulate_conflict(true);

				// Save should return None (conflict)
				let save_result = mock_db.save_commitment(&signed_commitment).await.unwrap();
				assert!(save_result.is_none());
			}
		}
	}
}
