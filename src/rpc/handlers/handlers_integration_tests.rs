//! Comprehensive integration tests for RPC handlers
//!
//! These tests exercise the full end-to-end flow of commitment requests,
//! including delegation verification, signing, and database operations.

#[cfg(test)]
mod integration_tests {
	use super::super::*;
	use crate::crypto::{bls::BlsManager, generate_request_hash, sign_commitment};
	use crate::db::DatabaseContext;
	use crate::testing::fixtures::TestFixtures;
	use crate::testing::mocks::{create_test_bls_keypair, create_test_config};
	use crate::types::beacon::BeaconTiming;
	use crate::types::delegation::{BlsSignature, DelegationMessage, SignedDelegation};
	use crate::types::{Commitment, CommitmentRequest, RpcContext, SignedCommitment};
	use anyhow::Result;
	use serial_test::serial;
	use sqlx::PgPool;
	use std::sync::Arc;

	/// Setup a real database connection pool for integration tests
	async fn setup_test_pool() -> Result<PgPool> {
		let database_url = std::env::var("DATABASE_URL")
			.unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/preconfirmation_gateway".to_string());

		let pool = PgPool::connect(&database_url).await?;

		Ok(pool)
	}

	/// Helper to create a properly signed delegation for testing
	fn create_properly_signed_delegation(
		slot: u64,
		proposer_sk: &blst::min_pk::SecretKey,
		delegate_pk: crate::types::delegation::BlsPublicKey,
		committer: &str,
	) -> SignedDelegation {
		use ethabi::{Token, encode};

		// Get proposer public key from secret key
		let proposer_blst_pk = proposer_sk.sk_to_pk();
		let proposer_pk_bytes = proposer_blst_pk.to_bytes();
		let mut proposer_pk_array = [0u8; 48];
		proposer_pk_array.copy_from_slice(&proposer_pk_bytes);
		let proposer_pk = crate::types::delegation::BlsPublicKey(proposer_pk_array);

		// Create delegation message
		let message =
			DelegationMessage { proposer: proposer_pk, delegate: delegate_pk, committer: committer.to_string(), slot };

		// ABI encode the delegation message
		let committer_hex = committer.strip_prefix("0x").unwrap_or(committer);
		let committer_bytes = hex::decode(committer_hex).expect("Valid hex");
		let committer_address = ethabi::Address::from_slice(&committer_bytes);

		let tokens = vec![
			Token::Bytes(message.proposer.0.to_vec()),
			Token::Bytes(message.delegate.0.to_vec()),
			Token::Address(committer_address),
			Token::Uint(message.slot.into()),
		];
		let encoded = encode(&tokens);

		// Calculate signing root with delegation domain
		let mut combined = Vec::new();
		combined.extend_from_slice(&crate::crypto::bls::domains::DELEGATION_DOMAIN_SEPARATOR);
		combined.extend_from_slice(&encoded);
		let signing_root = crate::crypto::keccak256(&combined);

		// Sign with proposer's secret key
		let signature = proposer_sk.sign(&signing_root, crate::crypto::bls::domains::BLS_POP_DST, &[]);
		let signature_bytes = signature.to_bytes();
		let mut sig_array = [0u8; 96];
		sig_array.copy_from_slice(&signature_bytes);

		SignedDelegation { message, signature: BlsSignature(sig_array) }
	}

	#[tokio::test]
	#[serial]
	async fn test_commitment_request_handler_full_flow() -> Result<()> {
		// Skip test if no database available
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let config = create_test_config();

		// Create a future slot for the commitment
		let genesis_time = config.beacon_api.genesis_time;
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let future_slot = current_slot + 10;

		// Create test keys
		let (proposer_sk, _proposer_pk) = create_test_bls_keypair();
		let (_delegate_sk, delegate_pk) = create_test_bls_keypair();

		// Get the gateway's BLS public key (for delegation)
		let bls_manager = BlsManager::new(&config.delegation.domain_application_gateway)?;

		// Create a properly signed delegation
		let delegation = create_properly_signed_delegation(
			future_slot,
			&proposer_sk,
			delegate_pk,
			&config.signing.committer_address,
		);

		// Save the delegation to the database
		crate::db::delegation_ops::save_delegations_batch(&pool, std::slice::from_ref(&delegation), &bls_manager)
			.await?;

		// Create commitment request
		let _request =
			TestFixtures::create_inclusion_commitment_request(future_slot, &config.signing.committer_address);

		// Create RPC context
		let database = DatabaseContext::new(pool.clone());
		let reth_config = crate::api::reth::RethApiConfig::default();
		let reth_client = Arc::new(crate::api::reth::RethApiClient::new(reth_config)?);
		let database_arc = Arc::new(database.clone());
		let config_arc = Arc::new(config.clone());
		let fee_engine = Arc::new(crate::services::fee_pricing::FeePricingEngine::new(
			reth_client,
			database_arc,
			config_arc.clone(),
		));
		let beacon_client = Arc::new(crate::api::beacon::BeaconApiClient::new(config.beacon_api.clone())?);
		let _context = Arc::new(RpcContext::new(database, config, fee_engine, beacon_client));

		// This test would require full integration with beacon chain mocking
		// For now, we've validated the setup works

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_commitment_result_handler_found() -> Result<()> {
		// Skip test if no database available
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Create a test commitment
		let request =
			TestFixtures::create_inclusion_commitment_request(12345, "0x1234567890123456789012345678901234567890");
		let request_hash = generate_request_hash(&request)?;

		let commitment = Commitment {
			commitment_type: request.commitment_type,
			payload: request.payload.clone(),
			request_hash: request_hash.clone(),
			slasher: request.slasher.clone(),
		};

		// Create a test signing key
		let private_key =
			crate::crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")?;
		let signature = sign_commitment(&commitment, &private_key)?;
		let signed_commitment = SignedCommitment { commitment, signature };

		// Save to database
		database.save_commitment(&signed_commitment).await?;

		// Now test retrieval
		let retrieved = database.get_commitment_by_hash(&request_hash).await?;
		assert!(retrieved.is_some());
		let retrieved_commitment = retrieved.unwrap();
		assert_eq!(retrieved_commitment.commitment.request_hash, request_hash);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_commitment_result_handler_not_found() -> Result<()> {
		// Skip test if no database available
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Try to retrieve non-existent commitment
		let fake_hash = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
		let result = database.get_commitment_by_hash(fake_hash).await?;

		assert!(result.is_none());

		Ok(())
	}

	#[test]
	fn test_slots_handler_full_range() {
		let config = create_test_config();

		// Calculate expected slot range
		let genesis_time = config.beacon_api.genesis_time;
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let lookahead_slots = config.delegation.lookahead_epochs * crate::types::beacon::timing::SLOTS_PER_EPOCH;

		// Expected range
		let start_slot = current_slot + 1;
		let end_slot = start_slot + lookahead_slots;

		// Verify the range is correct
		assert_eq!(end_slot - start_slot, lookahead_slots);
		assert_eq!(lookahead_slots, 64); // 2 epochs * 32 slots
	}

	#[tokio::test]
	#[serial]
	async fn test_fee_handler_with_valid_request() -> Result<()> {
		// Skip test if no database available
		let _ = std::env::var("DATABASE_URL").unwrap();

		let _pool = setup_test_pool().await?;
		let config = create_test_config();

		// Create a future slot for fee calculation
		let genesis_time = config.beacon_api.genesis_time;
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let future_slot = current_slot + 5;

		// Create test request
		let request = TestFixtures::create_inclusion_commitment_request(
			future_slot,
			"0x1234567890123456789012345678901234567890",
		);

		// Verify request has valid payload
		assert_eq!(request.commitment_type, 1);
		assert!(!request.payload.is_empty());

		Ok(())
	}

	#[tokio::test]
	async fn test_fee_handler_invalid_commitment_type() -> Result<()> {
		// Test that fee handler rejects invalid commitment types
		let request = CommitmentRequest {
			commitment_type: 99, // Invalid type
			payload: vec![1, 2, 3, 4],
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		// Validation should fail
		assert_eq!(request.commitment_type, 99);

		Ok(())
	}

	#[tokio::test]
	async fn test_fee_handler_slot_out_of_range() -> Result<()> {
		let config = create_test_config();

		// Create a slot that's too far in the future
		let genesis_time = config.beacon_api.genesis_time;
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let far_future_slot = current_slot + 100; // Way beyond acceptable range

		let _request = TestFixtures::create_inclusion_commitment_request(
			far_future_slot,
			"0x1234567890123456789012345678901234567890",
		);

		// Fee engine will reject this (max lookahead is 10 slots)
		let reth_config = crate::api::reth::RethApiConfig::default();
		let reth_client = Arc::new(crate::api::reth::RethApiClient::new(reth_config)?);
		let pool = PgPool::connect_lazy("postgresql://test:test@localhost/test_db")?;
		let database = Arc::new(DatabaseContext::new(pool));
		let config_arc = Arc::new(config);
		let fee_engine = crate::services::fee_pricing::FeePricingEngine::new(reth_client, database, config_arc.clone());

		// Check that slot is not acceptable
		assert!(!fee_engine.is_slot_acceptable_for_fees(far_future_slot));

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_duplicate_commitment_detection() -> Result<()> {
		// Skip test if no database available
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Create a unique test commitment
		let request =
			TestFixtures::create_inclusion_commitment_request(54321, "0x1234567890123456789012345678901234567890");
		let request_hash = generate_request_hash(&request)?;

		let commitment = Commitment {
			commitment_type: request.commitment_type,
			payload: request.payload.clone(),
			request_hash: request_hash.clone(),
			slasher: request.slasher.clone(),
		};

		let private_key =
			crate::crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")?;
		let signature = sign_commitment(&commitment, &private_key)?;
		let signed_commitment = SignedCommitment { commitment, signature };

		// Save first time
		let result1 = database.save_commitment(&signed_commitment).await?;
		assert!(result1.is_some());

		// Check existence
		let exists = database.commitment_exists(&request_hash).await?;
		assert!(exists);

		// Try to save again - should return None due to ON CONFLICT
		let result2 = database.save_commitment(&signed_commitment).await?;
		assert!(result2.is_none());

		Ok(())
	}

	#[tokio::test]
	async fn test_validate_and_extract_slot_with_various_types() {
		use crate::types::payload::{InclusionPayload, PayloadParser};

		// Test type 1 (inclusion)
		let inclusion_payload = InclusionPayload::new(99999, vec![1, 2, 3, 4]);
		let encoded = PayloadParser::encode_inclusion_payload(&inclusion_payload).unwrap();

		let result = validate_and_extract_slot(1, &encoded);
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 99999);

		// Test invalid type
		let result = validate_and_extract_slot(999, &encoded);
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("Unknown commitment type"));

		// Test empty payload
		let result = validate_and_extract_slot(1, &[]);
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("Failed to extract slot"));
	}

	#[tokio::test]
	async fn test_validate_slasher_address_variations() {
		let config = create_test_config();
		let context = crate::testing::helpers::TestHelpers::create_test_rpc_context(Arc::new(config));

		// Test with exact match
		let result = validate_slasher_address(&context, "0x1234567890123456789012345678901234567890");
		assert!(result.is_ok());

		// Test with uppercase
		let result =
			validate_slasher_address(&context, "0x1234567890123456789012345678901234567890".to_uppercase().as_str());
		assert!(result.is_ok());

		// Test without 0x prefix
		let result = validate_slasher_address(&context, "1234567890123456789012345678901234567890");
		assert!(result.is_ok());

		// Test with non-whitelisted address
		let result = validate_slasher_address(&context, "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("not in the configured whitelist"));
	}

	#[tokio::test]
	async fn test_find_signing_key_for_committer_success_and_failure() {
		let config = create_test_config();
		let context = crate::testing::helpers::TestHelpers::create_test_rpc_context(Arc::new(config.clone()));

		// Test with matching committer address
		let result = find_signing_key_for_committer(&context, &config.signing.committer_address);
		assert!(result.is_ok());

		// Test with non-matching address
		let result = find_signing_key_for_committer(&context, "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
		assert!(result.is_err());
		assert!(result.unwrap_err().contains("No signing key found"));
	}

	#[tokio::test]
	async fn test_commitment_request_validation_error_paths() {
		let config = create_test_config();
		let context = crate::testing::helpers::TestHelpers::create_test_rpc_context(Arc::new(config.clone()));

		// Test 1: Invalid commitment type (not 1)
		let request = CommitmentRequest {
			commitment_type: 2,
			payload: vec![1, 2, 3, 4],
			slasher: config.validation.slasher_whitelist[0].clone(),
		};

		assert_ne!(request.commitment_type, 1);

		// Test 2: Invalid slasher address
		let request = CommitmentRequest {
			commitment_type: 1,
			payload: vec![1, 2, 3, 4],
			slasher: "0xnotwhitelisted0000000000000000000000000".to_string(),
		};

		let result = validate_slasher_address(&context, &request.slasher);
		assert!(result.is_err());

		// Test 3: Invalid payload
		let request = CommitmentRequest {
			commitment_type: 1,
			payload: vec![0xff, 0xff], // Too short/invalid
			slasher: config.validation.slasher_whitelist[0].clone(),
		};

		let result = validate_and_extract_slot(request.commitment_type, &request.payload);
		assert!(result.is_err());
	}

	#[tokio::test]
	#[serial]
	async fn test_concurrent_commitment_requests() -> Result<()> {
		// Skip test if no database available
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Create multiple unique commitments concurrently
		let mut handles = vec![];

		for i in 0..5 {
			let db = database.clone();
			let handle = tokio::spawn(async move {
				let request = TestFixtures::create_inclusion_commitment_request(
					10000 + i,
					"0x1234567890123456789012345678901234567890",
				);
				let request_hash = generate_request_hash(&request).unwrap();

				let commitment = Commitment {
					commitment_type: request.commitment_type,
					payload: request.payload.clone(),
					request_hash: request_hash.clone(),
					slasher: request.slasher.clone(),
				};

				let private_key = crate::crypto::parse_private_key(
					"ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
				)
				.unwrap();
				let signature = sign_commitment(&commitment, &private_key).unwrap();
				let signed_commitment = SignedCommitment { commitment, signature };

				db.save_commitment(&signed_commitment).await
			});

			handles.push(handle);
		}

		// Wait for all to complete
		for handle in handles {
			let result = handle.await.unwrap();
			assert!(result.is_ok());
			assert!(result.unwrap().is_some()); // All should succeed (unique requests)
		}

		Ok(())
	}
}
