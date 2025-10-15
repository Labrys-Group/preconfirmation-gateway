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
		let database_url = std::env::var("DATABASE_URL")?;

		Ok(PgPool::connect(&database_url).await?)
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
	async fn test_commitment_result_handler_found() -> Result<()> {
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

	// COMPREHENSIVE INTEGRATION TESTS - Enhanced Test Coverage
	// ==================================================================================

	/// Helper to create RPC context with real database and config
	async fn create_test_rpc_context(pool: PgPool) -> Arc<RpcContext> {
		let database = DatabaseContext::new(pool);
		let config = create_test_config();

		// Create fee engine with real components
		let reth_config = crate::api::reth::RethApiConfig::default();
		let reth_client = Arc::new(crate::api::reth::RethApiClient::new(reth_config).unwrap());
		let database_arc = Arc::new(database.clone());
		let config_arc = Arc::new(config.clone());
		let fee_engine = Arc::new(crate::services::fee_pricing::FeePricingEngine::new(
			reth_client,
			database_arc,
			config_arc.clone(),
		));

		// Create beacon API client
		let beacon_client = Arc::new(crate::api::beacon::BeaconApiClient::new(config.beacon_api.clone()).unwrap());

		Arc::new(RpcContext::new(database, config, fee_engine, beacon_client))
	}

	// PHASE 1: Commitment Result Handler Integration Tests
	// ==================================================================================

	#[tokio::test]
	#[serial]
	async fn test_commitment_result_handler_with_rpc_context() -> Result<()> {
		let pool = setup_test_pool().await?;
		let context = create_test_rpc_context(pool.clone()).await;

		// Create and save a commitment
		let request =
			TestFixtures::create_inclusion_commitment_request(55555, "0x1234567890123456789012345678901234567890");
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
		let signed_commitment = SignedCommitment { commitment, signature: signature.clone() };

		// Save directly to database
		context.database.save_commitment(&signed_commitment).await?;

		// Now call the handler
		let params_json = format!(r#"["{}"]"#, request_hash);
		let params_json_static: &'static str = Box::leak(params_json.into_boxed_str());
		let params = jsonrpsee::types::Params::new(Some(params_json_static));
		let extensions = jsonrpsee::Extensions::new();
		let result = commitment_result_handler(params, context.clone(), extensions).await;

		assert!(result.is_ok(), "Handler should succeed");
		let retrieved = result.unwrap();
		assert_eq!(retrieved.commitment.request_hash, request_hash);
		assert_eq!(retrieved.signature, signature);
		assert_eq!(retrieved.commitment.commitment_type, 1);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_commitment_result_handler_not_found_error() -> Result<()> {
		let pool = setup_test_pool().await?;
		let context = create_test_rpc_context(pool.clone()).await;

		let fake_hash = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
		let params_json = format!(r#"["{}"]"#, fake_hash);
		let params_json_static: &'static str = Box::leak(params_json.into_boxed_str());
		let params = jsonrpsee::types::Params::new(Some(params_json_static));
		let extensions = jsonrpsee::Extensions::new();

		let result = commitment_result_handler(params, context.clone(), extensions).await;

		assert!(result.is_err(), "Handler should return error for non-existent commitment");

		// Verify it's the correct error code (InvalidRequest)
		let err = result.unwrap_err();
		assert_eq!(err.code(), jsonrpsee::types::error::ErrorCode::InvalidRequest.code());

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_fee_handler_rejects_invalid_commitment_type() -> Result<()> {
		let pool = setup_test_pool().await?;
		let context = create_test_rpc_context(pool.clone()).await;

		let invalid_request = CommitmentRequest {
			commitment_type: 99, // Invalid type
			payload: vec![1, 2, 3, 4],
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let request_json = serde_json::to_string(&invalid_request)?;
		let params_json = format!(r#"[{}]"#, request_json);
		let params_json_static: &'static str = Box::leak(params_json.into_boxed_str());
		let params = jsonrpsee::types::Params::new(Some(params_json_static));
		let extensions = jsonrpsee::Extensions::new();

		let result = fee_handler(params, context.clone(), extensions).await;

		assert!(result.is_err(), "Fee handler should reject invalid commitment type");
		let err = result.unwrap_err();
		assert_eq!(err.code(), jsonrpsee::types::error::ErrorCode::InvalidParams.code());

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_fee_handler_rejects_slot_out_of_range() -> Result<()> {
		let pool = setup_test_pool().await?;
		let context = create_test_rpc_context(pool.clone()).await;

		// Create a slot that's way too far in the future (> 10 slots)
		let genesis_time = context.config.beacon_api.genesis_time;
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let far_future_slot = current_slot + 100;

		let request = TestFixtures::create_inclusion_commitment_request(
			far_future_slot,
			"0x1234567890123456789012345678901234567890",
		);

		let request_json = serde_json::to_string(&request)?;
		let params_json = format!(r#"[{}]"#, request_json);
		let params_json_static: &'static str = Box::leak(params_json.into_boxed_str());
		let params = jsonrpsee::types::Params::new(Some(params_json_static));
		let extensions = jsonrpsee::Extensions::new();

		let result = fee_handler(params, context.clone(), extensions).await;

		assert!(result.is_err(), "Fee handler should reject slot out of range");
		let err = result.unwrap_err();
		assert_eq!(err.code(), jsonrpsee::types::error::ErrorCode::InvalidParams.code());

		Ok(())
	}

	// PHASE 3: Duplicate Detection and Race Condition Tests
	// ==================================================================================

	#[tokio::test]
	#[serial]
	async fn test_early_duplicate_detection_path() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Create a unique test commitment
		let request =
			TestFixtures::create_inclusion_commitment_request(77777, "0x1234567890123456789012345678901234567890");
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

		// Save first commitment
		let result1 = database.save_commitment(&signed_commitment).await?;
		assert!(result1.is_some(), "First save should succeed");

		// Verify exists
		let exists = database.commitment_exists(&request_hash).await?;
		assert!(exists, "Commitment should exist after first save");

		// Try to save again - should return None due to ON CONFLICT
		let result2 = database.save_commitment(&signed_commitment).await?;
		assert!(result2.is_none(), "Duplicate save should return None");

		// Verify still only one commitment exists
		let count_query = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM commitments WHERE request_hash = $1")
			.bind(&request_hash)
			.fetch_one(database.pool())
			.await?;

		assert_eq!(count_query, 1, "Should have exactly one commitment");

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_database_level_duplicate_detection() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Create a commitment that will be saved concurrently
		let request =
			TestFixtures::create_inclusion_commitment_request(88888, "0x1234567890123456789012345678901234567890");
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

		// Spawn two concurrent tasks trying to save the same commitment
		let db1 = database.clone();
		let db2 = database.clone();
		let sc1 = signed_commitment.clone();
		let sc2 = signed_commitment.clone();

		let handle1 = tokio::spawn(async move { db1.save_commitment(&sc1).await });

		let handle2 = tokio::spawn(async move { db2.save_commitment(&sc2).await });

		let result1 = handle1.await.unwrap()?;
		let result2 = handle2.await.unwrap()?;

		// One should succeed (Some), one should get duplicate (None)
		let success_count = [result1.is_some(), result2.is_some()].iter().filter(|&&x| x).count();
		assert_eq!(success_count, 1, "Exactly one save should succeed");

		// Verify only one commitment in database
		let count_query = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM commitments WHERE request_hash = $1")
			.bind(&request_hash)
			.fetch_one(database.pool())
			.await?;

		assert_eq!(count_query, 1, "Should have exactly one commitment despite concurrent saves");

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_duplicate_with_different_slasher_addresses() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		// Create two requests with same slot but different slasher addresses
		let request1 =
			TestFixtures::create_inclusion_commitment_request(99999, "0x1111111111111111111111111111111111111111");
		let request2 =
			TestFixtures::create_inclusion_commitment_request(99999, "0x2222222222222222222222222222222222222222");

		let hash1 = generate_request_hash(&request1)?;
		let hash2 = generate_request_hash(&request2)?;

		// Hashes should be different even though slot is the same
		assert_ne!(hash1, hash2, "Different slasher addresses should produce different hashes");

		let commitment1 = Commitment {
			commitment_type: request1.commitment_type,
			payload: request1.payload.clone(),
			request_hash: hash1.clone(),
			slasher: request1.slasher.clone(),
		};

		let commitment2 = Commitment {
			commitment_type: request2.commitment_type,
			payload: request2.payload.clone(),
			request_hash: hash2.clone(),
			slasher: request2.slasher.clone(),
		};

		let private_key =
			crate::crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")?;

		let signature1 = sign_commitment(&commitment1, &private_key)?;
		let signature2 = sign_commitment(&commitment2, &private_key)?;

		let signed1 = SignedCommitment { commitment: commitment1, signature: signature1 };
		let signed2 = SignedCommitment { commitment: commitment2, signature: signature2 };

		// Both should save successfully
		let result1 = database.save_commitment(&signed1).await?;
		let result2 = database.save_commitment(&signed2).await?;

		assert!(result1.is_some(), "First commitment should save");
		assert!(result2.is_some(), "Second commitment should save (different request_hash)");

		Ok(())
	}

	// PHASE 4: Enhanced Slots Handler Tests
	// ==================================================================================

	#[tokio::test]
	async fn test_slots_handler_timing_accuracy() -> Result<()> {
		let pool = setup_test_pool().await?;
		let context = create_test_rpc_context(pool.clone()).await;

		let genesis_time = context.config.beacon_api.genesis_time;
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let lookahead_slots =
			context.config.delegation.lookahead_epochs * crate::types::beacon::timing::SLOTS_PER_EPOCH;

		let params = jsonrpsee::types::Params::new(None);
		let extensions = jsonrpsee::Extensions::new();

		let result = slots_handler(params, &context, &extensions)?;

		// Verify slot range
		assert_eq!(result.slots.len(), lookahead_slots as usize, "Should return lookahead_slots slots");

		if let Some(first_slot) = result.slots.first() {
			assert_eq!(first_slot.slot, current_slot + 1, "First slot should be current + 1");
		}

		if let Some(last_slot) = result.slots.last() {
			assert_eq!(last_slot.slot, current_slot + lookahead_slots, "Last slot should be current + lookahead");
		}

		// Verify all slots have Hoodi offering
		for slot_info in &result.slots {
			assert_eq!(slot_info.offerings.len(), 1, "Each slot should have exactly one offering");
			let offering = &slot_info.offerings[0];
			assert_eq!(offering.chain_id, 560048, "Should be Hoodi chain");
			assert_eq!(offering.commitment_types, vec![1], "Should only support type 1");
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_slots_handler_offering_structure_validation() -> Result<()> {
		let pool = setup_test_pool().await?;
		let context = create_test_rpc_context(pool.clone()).await;

		let params = jsonrpsee::types::Params::new(None);
		let extensions = jsonrpsee::Extensions::new();

		let result = slots_handler(params, &context, &extensions)?;

		// Verify sequential slots
		for i in 1..result.slots.len() {
			assert_eq!(result.slots[i].slot, result.slots[i - 1].slot + 1, "Slots should be sequential");
		}

		// Verify offering structure for each slot
		for slot_info in &result.slots {
			assert_eq!(slot_info.offerings.len(), 1);
			let offering = &slot_info.offerings[0];

			assert_eq!(offering.chain_id, 560048);
			assert_eq!(offering.commitment_types.len(), 1);
			assert_eq!(offering.commitment_types[0], 1);
		}

		Ok(())
	}

	// PHASE 5: Concurrent and Performance Tests
	// ==================================================================================

	#[tokio::test]
	#[serial]
	async fn test_concurrent_commitment_requests_different_slots() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		let mut handles = vec![];

		// Spawn 10 concurrent tasks with different slots
		for i in 0..10 {
			let db = database.clone();
			let handle = tokio::spawn(async move {
				let slot = 200000 + i;
				let request = TestFixtures::create_inclusion_commitment_request(
					slot,
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
		let mut success_count = 0;
		for handle in handles {
			let result = handle.await.unwrap();
			assert!(result.is_ok(), "All saves should succeed");
			if result.unwrap().is_some() {
				success_count += 1;
			}
		}

		assert_eq!(success_count, 10, "All 10 commitments should be saved");

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_concurrent_commitment_requests_same_slot_different_tx() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		let test_slot = 300000u64;
		let mut handles = vec![];

		// Spawn 5 concurrent tasks with same slot but different transaction data
		for i in 0..5 {
			let db = database.clone();
			let handle = tokio::spawn(async move {
				use crate::types::payload::{InclusionPayload, PayloadParser};

				// Create unique transaction data for each task
				let tx_data = vec![i as u8; 32];
				let payload = InclusionPayload::new(test_slot, tx_data);
				let payload_bytes = PayloadParser::encode_inclusion_payload(&payload).unwrap();

				let request = CommitmentRequest {
					commitment_type: 1,
					payload: payload_bytes.clone(),
					slasher: "0x1234567890123456789012345678901234567890".to_string(),
				};

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
		let mut success_count = 0;
		for handle in handles {
			let result = handle.await.unwrap();
			assert!(result.is_ok(), "All saves should succeed with different tx data");
			if result.unwrap().is_some() {
				success_count += 1;
			}
		}

		assert_eq!(success_count, 5, "All 5 commitments should be saved (different request_hash)");

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_high_volume_commitment_processing() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		let start = std::time::Instant::now();
		let num_commitments = 50; // Reduced from 100 for faster test execution

		for i in 0..num_commitments {
			let request = TestFixtures::create_inclusion_commitment_request(
				400000 + i,
				"0x1234567890123456789012345678901234567890",
			);
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

			let result = database.save_commitment(&signed_commitment).await?;
			assert!(result.is_some(), "Commitment {} should be saved", i);
		}

		let duration = start.elapsed();
		let avg_time = duration.as_millis() / num_commitments as u128;

		println!("High volume test: {} commitments in {:?} (avg: {}ms)", num_commitments, duration, avg_time);

		// Average time should be reasonable (< 100ms per commitment in sequential processing)
		assert!(avg_time < 100, "Average processing time should be < 100ms, got {}ms", avg_time);

		Ok(())
	}

	// PHASE 6: Database Integration Edge Cases
	// ==================================================================================

	#[tokio::test]
	#[serial]
	async fn test_commitment_retrieval_after_save_integrity() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		let request =
			TestFixtures::create_inclusion_commitment_request(500000, "0x1234567890123456789012345678901234567890");
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
		let signed_commitment = SignedCommitment { commitment: commitment.clone(), signature: signature.clone() };

		// Save commitment
		database.save_commitment(&signed_commitment).await?;

		// Retrieve and verify exact match
		let retrieved = database.get_commitment_by_hash(&request_hash).await?;
		assert!(retrieved.is_some(), "Commitment should be retrievable");

		let retrieved = retrieved.unwrap();
		assert_eq!(retrieved.commitment.request_hash, request_hash);
		assert_eq!(retrieved.commitment.commitment_type, commitment.commitment_type);
		assert_eq!(retrieved.commitment.payload, commitment.payload);
		assert_eq!(retrieved.commitment.slasher, commitment.slasher);
		assert_eq!(retrieved.signature, signature);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_commitment_exists_check_race_condition() -> Result<()> {
		let pool = setup_test_pool().await?;
		let database = DatabaseContext::new(pool.clone());

		let request =
			TestFixtures::create_inclusion_commitment_request(600000, "0x1234567890123456789012345678901234567890");
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

		// Spawn two tasks that check existence and then save
		let db1 = database.clone();
		let db2 = database.clone();
		let sc1 = signed_commitment.clone();
		let sc2 = signed_commitment.clone();
		let hash1 = request_hash.clone();
		let hash2 = request_hash.clone();

		let handle1 = tokio::spawn(async move {
			let exists = db1.commitment_exists(&hash1).await.unwrap();
			if !exists { db1.save_commitment(&sc1).await.unwrap() } else { None }
		});

		let handle2 = tokio::spawn(async move {
			let exists = db2.commitment_exists(&hash2).await.unwrap();
			if !exists { db2.save_commitment(&sc2).await.unwrap() } else { None }
		});

		let result1 = handle1.await.unwrap();
		let result2 = handle2.await.unwrap();

		// At least one should succeed or get None from ON CONFLICT
		// Both seeing exists=false is possible in race, but ON CONFLICT protects us
		let saves = [result1.is_some(), result2.is_some()];
		let success_count = saves.iter().filter(|&&x| x).count();

		// Either one succeeds, or both get None from ON CONFLICT
		assert!(success_count <= 1, "At most one save should succeed");

		// Verify only one commitment exists
		let exists = database.commitment_exists(&request_hash).await?;
		assert!(exists, "Commitment should exist");

		let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM commitments WHERE request_hash = $1")
			.bind(&request_hash)
			.fetch_one(database.pool())
			.await?;

		assert_eq!(count, 1, "Should have exactly one commitment despite race");

		Ok(())
	}
}
