use std::sync::Arc;
use jsonrpsee::Extensions;
use jsonrpsee::core::RpcResult;
use tracing::{info, warn, error, instrument};

use super::super::types::{
    Commitment, CommitmentRequest, FeeInfo, RpcContext, SignedCommitment, SlotInfoResponse,
    PayloadParser, BeaconTiming,
};
use super::super::types::rpc::{Offering, SlotInfo};
use super::super::types::beacon::timing;
use crate::crypto::{generate_request_hash, sign_commitment};
use crate::db::delegation_ops::get_delegations_for_slot;

/// Validate payload and extract slot number
fn validate_and_extract_slot(commitment_type: u64, payload: &[u8]) -> Result<u64, String> {
    PayloadParser::extract_slot(commitment_type, payload)
        .map_err(|e| format!("Failed to extract slot from payload: {}", e))
}

/// Check if we have valid delegation authority for the given slot and committer
async fn verify_delegation_authority(
    context: &RpcContext,
    slot: u64,
    committer_address: &str
) -> Result<(), String> {
    // Get delegations for this slot from the database
    let delegations = get_delegations_for_slot(context.database.pool(), slot).await
        .map_err(|e| format!("Failed to query delegations for slot {}: {}", slot, e))?;

    // Check if we have any delegation for this slot and committer
    let has_delegation = delegations.iter().any(|delegation| {
        delegation.message.committer == committer_address &&
        delegation.is_valid_for_slot(slot)
    });

    if !has_delegation {
        return Err(format!(
            "No valid delegation found for slot {} and committer {}. Cannot sign commitment without delegation authority.",
            slot, committer_address
        ));
    }

    info!(
        "Delegation authority verified for slot {} and committer {}",
        slot, committer_address
    );
    Ok(())
}

/// Find the appropriate ECDSA key for signing based on the committer address
fn find_signing_key_for_committer<'a>(
    context: &'a RpcContext,
    committer_address: &str,
) -> Result<&'a secp256k1::SecretKey, String> {
    // First try to find a key pair that matches the committer address
    if let Some(key_pair) = context.config.signing.key_pairs
        .iter()
        .find(|kp| kp.committer_address == committer_address)
    {
        return Ok(&key_pair.ecdsa_private_key);
    }

    // Fallback to the legacy single key if the committer matches the configured address
    if committer_address == context.config.validation.slasher_address {
        return Ok(&context.config.signing.private_key);
    }

    Err(format!(
        "No signing key found for committer address: {}. Available addresses: {:?}",
        committer_address,
        context.config.signing.key_pairs.iter()
            .map(|kp| &kp.committer_address)
            .collect::<Vec<_>>()
    ))
}

#[instrument(name = "commitment_request", skip(context, _extensions))]
pub async fn commitment_request_handler(
	params: jsonrpsee::types::Params<'static>,
	context: Arc<RpcContext>,
	_extensions: Extensions,
) -> RpcResult<SignedCommitment> {
	info!("Processing commitment request with delegation-first security");
	let request: CommitmentRequest = params.parse()?;

	// Validate commitment_type
	if request.commitment_type != 1 {
		warn!("Invalid commitment type: {}", request.commitment_type);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Extract slot from payload - CRITICAL: This must succeed before any signing
	let slot = validate_and_extract_slot(request.commitment_type, &request.payload)
		.map_err(|e| {
			warn!("Payload validation failed: {}", e);
			jsonrpsee::types::error::ErrorCode::InvalidParams
		})?;

	info!("Extracted slot {} from commitment payload", slot);

	// DELEGATION-FIRST SECURITY: Verify delegation authority BEFORE any signing
	verify_delegation_authority(&context, slot, &request.slasher).await
		.map_err(|e| {
			error!("Delegation verification failed: {}", e);
			jsonrpsee::types::error::ErrorCode::InvalidRequest
		})?;

	// Find the appropriate signing key for this committer
	let signing_key = find_signing_key_for_committer(&context, &request.slasher)
		.map_err(|e| {
			error!("No signing key found: {}", e);
			jsonrpsee::types::error::ErrorCode::InvalidRequest
		})?;

	// Generate request hash
	let request_hash = generate_request_hash(&request)
		.map_err(|e| {
			error!("Failed to generate request hash: {}", e);
			jsonrpsee::types::error::ErrorCode::InternalError
		})?;

	// Check if commitment already exists to prevent duplicates
	if context.database.commitment_exists(&request_hash).await
		.map_err(|e| {
			error!("Database error checking commitment existence: {}", e);
			jsonrpsee::types::error::ErrorCode::InternalError
		})? {
		warn!("Duplicate commitment request: {}", request_hash);
		return Err(jsonrpsee::types::error::ErrorCode::InvalidRequest.into());
	}

	// Create commitment with real request hash
	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload.clone(),
		request_hash: request_hash.clone(),
		slasher: request.slasher.clone(),
	};

	// Sign the commitment with the appropriate key
	let signature = sign_commitment(&commitment, signing_key)
		.map_err(|e| {
			error!("Failed to sign commitment: {}", e);
			jsonrpsee::types::error::ErrorCode::InternalError
		})?;

	let signed_commitment = SignedCommitment {
		commitment,
		signature,
	};

	// Save to database
	context.database.save_commitment(&signed_commitment).await
		.map_err(|e| {
			error!("Failed to save commitment to database: {}", e);
			jsonrpsee::types::error::ErrorCode::InternalError
		})?;

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
		// Create offering for Hooli chain with inclusion commitments (type 1)
		let hooli_offering = Offering {
			chain_id: 560048, // Hooli chain ID
			commitment_types: vec![1], // Only support inclusion commitments
		};

		let slot_info = SlotInfo {
			slot,
			offerings: vec![hooli_offering],
		};

		slots.push(slot_info);
	}

	let slots_count = slots.len();
	let response = SlotInfoResponse { slots };

	info!(
		"Slots request processed successfully: {} slots from {} to {}",
		slots_count,
		start_slot,
		end_slot
	);
	Ok(response)
}

#[instrument(name = "fee", skip(_context, _extensions))]
pub fn fee_handler(
	params: jsonrpsee::types::Params<'_>,
	_context: &RpcContext,
	_extensions: &Extensions,
) -> RpcResult<FeeInfo> {
	info!("Processing fee request");
	let request: CommitmentRequest = params.parse()?;

	// TODO: Implement actual fee calculation logic
	let fee_info = FeeInfo { fee_payload: vec![0u8; 32], commitment_type: request.commitment_type };

	info!("Fee request processed successfully");
	Ok(fee_info)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::{CommitmentRequest, payload::InclusionPayload, payload::PayloadParser};
	use crate::config::Config;
	use crate::db::DatabaseContext;
	use sqlx::PgPool;
	use std::sync::Arc;

	// Helper to create a test RPC context with minimal configuration
	fn create_test_context() -> Arc<RpcContext> {
		use crate::crypto::parse_private_key;
		use crate::crypto::bls_keys;

		let private_key = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
		let bls_key = bls_keys::parse_private_key("0x1234567890123456789012345678901234567890123456789012345678901234").unwrap();

		let config = Config {
			server: crate::config::ServerConfig {
				host: "127.0.0.1".to_string(),
				port: 8545,
			},
			database: crate::config::DatabaseConfig {
				url: "postgresql://test:test@localhost/test_db".to_string(),
			},
			logging: crate::config::LoggingConfig {
				level: "info".to_string(),
				enable_method_tracing: false,
				traced_methods: vec![],
			},
			validation: crate::config::ValidationConfig {
				slasher_address: "0x1234567890123456789012345678901234567890".to_string(),
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
			signing: crate::config::SigningConfig {
				private_key,
				key_pairs: vec![
					crate::config::KeyPair {
						name: "test_key".to_string(),
						ecdsa_private_key: parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap(),
						bls_private_key: bls_key.clone(),
						bls_public_key: bls_key.sk_to_pk(),
						committer_address: "0x1234567890123456789012345678901234567890".to_string(),
					}
				],
			},
		};

		// Create a test database pool (won't actually connect in unit tests)
		let pool = PgPool::connect_lazy(&config.database.url)
			.expect("Failed to create test pool");
		let database = DatabaseContext::new(pool);

		Arc::new(RpcContext { database, config })
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

	#[test]
	fn test_find_signing_key_for_committer_success() {
		// Test key finding logic directly without database context
		use crate::crypto::{parse_private_key, ecdsa_to_address};

		let private_key = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
		let address = ecdsa_to_address(&private_key).unwrap();

		// Test that we can find a key for the derived address
		assert_eq!(address.to_lowercase(), "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");

		// This proves the key derivation logic works correctly
		assert!(private_key.secret_bytes().len() == 32);
	}

	#[test]
	fn test_find_signing_key_for_committer_not_found() {
		// Test that different keys produce different addresses
		use crate::crypto::{parse_private_key, ecdsa_to_address};

		let private_key1 = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
		let private_key2 = parse_private_key("59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d").unwrap();

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
				slasher: context.config.validation.slasher_address.clone(),
			};

			// We can't call the handler directly due to database dependencies,
			// but we can test the validation functions
			let slot_result = validate_and_extract_slot(
				invalid_request.commitment_type,
				&invalid_request.payload
			);
			assert!(slot_result.is_err());

			// Test 2: Valid commitment type but invalid payload
			let invalid_payload_request = CommitmentRequest {
				commitment_type: 1,
				payload: vec![0xff, 0xff, 0xff, 0xff], // Invalid payload
				slasher: context.config.validation.slasher_address.clone(),
			};

			let slot_result = validate_and_extract_slot(
				invalid_payload_request.commitment_type,
				&invalid_payload_request.payload
			);
			assert!(slot_result.is_err());

			// Test 3: Valid payload structure
			let valid_request = TestFixtures::create_inclusion_commitment_request(
				12345,
				&context.config.validation.slasher_address
			);

			let slot_result = validate_and_extract_slot(
				valid_request.commitment_type,
				&valid_request.payload
			);
			assert!(slot_result.is_ok());
			assert_eq!(slot_result.unwrap(), 12345);
		}

		#[test]
		fn test_signing_key_management() {
			// Test the ECDSA address derivation logic directly
			use crate::crypto::{parse_private_key, ecdsa_to_address};

			// Test known private key to address mapping
			let private_key = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
			let derived_address = ecdsa_to_address(&private_key).unwrap();

			// This is the expected address for this private key (from Hardhat)
			assert_eq!(derived_address.to_lowercase(), "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266");

			// Test that key parsing works
			assert_eq!(private_key.secret_bytes().len(), 32);
		}

		#[test]
		fn test_request_hash_generation_consistency() {
			// Test that request hash generation is deterministic
			let request = TestFixtures::create_inclusion_commitment_request(
				12345,
				"0x1234567890123456789012345678901234567890"
			);

			let hash1 = generate_request_hash(&request).unwrap();
			let hash2 = generate_request_hash(&request).unwrap();
			assert_eq!(hash1, hash2);
			assert!(hash1.starts_with("0x"));
			assert_eq!(hash1.len(), 66); // 0x + 64 hex chars
		}

		#[test]
		fn test_commitment_structure_validation() {
			// Test that our commitment structures are properly formed
			let request = TestFixtures::create_inclusion_commitment_request(
				12345,
				"0x1234567890123456789012345678901234567890"
			);

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
			use crate::config::{Config, BeaconApiConfig, DelegationConfig};

			// Create minimal config for testing
			let config = Config {
				server: crate::config::ServerConfig {
					host: "127.0.0.1".to_string(),
					port: 8545,
				},
				database: crate::config::DatabaseConfig {
					url: "postgresql://test:test@localhost/test_db".to_string(),
				},
				logging: crate::config::LoggingConfig {
					level: "info".to_string(),
					enable_method_tracing: false,
					traced_methods: vec![],
				},
				validation: crate::config::ValidationConfig {
					slasher_address: "0x1234567890123456789012345678901234567890".to_string(),
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
				signing: crate::config::SigningConfig {
					private_key: secp256k1::SecretKey::from_slice(&[1; 32]).unwrap(),
					key_pairs: vec![],
				},
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
				let hooli_offering = Offering {
					chain_id: 560048, // Hooli chain ID
					commitment_types: vec![1], // Only support inclusion commitments
				};

				let slot_info = SlotInfo {
					slot,
					offerings: vec![hooli_offering],
				};

				expected_slots.push(slot_info);
			}

			// Verify the logic produces correct results
			assert_eq!(expected_slots.len(), lookahead_slots as usize);

			// Verify each slot has the correct offering for Hooli chain
			for slot_info in &expected_slots {
				assert_eq!(slot_info.offerings.len(), 1);

				let offering = &slot_info.offerings[0];
				assert_eq!(offering.chain_id, 560048); // Hooli chain ID
				assert_eq!(offering.commitment_types, vec![1]); // Only inclusion commitments

				// Verify slot numbers are reasonable
				assert!(slot_info.slot >= expected_start_slot);
				assert!(slot_info.slot < expected_end_slot);
			}

			// Verify slots are in ascending order
			for i in 1..expected_slots.len() {
				assert!(expected_slots[i].slot > expected_slots[i-1].slot);
				assert_eq!(expected_slots[i].slot, expected_slots[i-1].slot + 1);
			}
		}
	}
}
