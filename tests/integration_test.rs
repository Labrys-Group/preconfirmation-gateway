//! Integration tests for signing and database operations
//!
//! These tests verify the complete functionality of the commitment system:
//! - ECDSA signing of commitments
//! - Database storage and retrieval
//! - End-to-end functionality

use std::sync::Arc;
use preconfirmation_gateway::{
	CommitmentRequest, Config, DatabaseContext, RpcContext,
	crypto::{generate_request_hash, sign_commitment, verify_commitment_signature},
	types::{Commitment, SignedCommitment}
};
use ethabi::{ParamType, encode, Token};
use sqlx::PgPool;
use secp256k1::Secp256k1;
use tokio;

/// Setup test database pool for integration tests
async fn setup_test_pool() -> PgPool {
	let database_url = std::env::var("TEST_DATABASE_URL")
		.unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/preconfirmation_gateway_test".to_string());

	// Create connection pool
	let pool = sqlx::postgres::PgPoolOptions::new()
		.max_connections(5)
		.connect(&database_url)
		.await
		.expect("Failed to create test database connection pool");

	// Run migrations
	sqlx::migrate!("./migrations")
		.run(&pool)
		.await
		.expect("Failed to run test database migrations");

	// Clean up any existing test data
	sqlx::query("DELETE FROM commitments")
		.execute(&pool)
		.await
		.expect("Failed to clean test database");

	pool
}

/// Create test RPC context with database and signing configuration
async fn create_test_context() -> Arc<RpcContext> {
	let pool = setup_test_pool().await;
	let db_context = DatabaseContext::new(pool);

	// Load config (uses TEST environment variables if available)
	let config = Config::load().expect("Failed to load test config");

	Arc::new(RpcContext::new(db_context, config))
}

/// Helper function to create a valid ABI-encoded InclusionPayload with unique nonce
fn create_valid_payload() -> Vec<u8> {
	use std::time::{SystemTime, UNIX_EPOCH};
	let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
	create_payload_with_nonce(nonce)
}

/// Helper function to create payload with specific nonce for uniqueness
fn create_payload_with_nonce(nonce: u64) -> Vec<u8> {
	let _types = vec![
		ParamType::FixedBytes(32), // tx_hash (Bytes32)
		ParamType::Uint(256),      // nonce (uint256)
		ParamType::Uint(256),      // gas_limit (uint256)
		ParamType::Uint(64),       // slot (uint64)
	];

	let tokens = vec![
		Token::FixedBytes(vec![0x9f, 0xbb].into_iter().chain(std::iter::repeat(0).take(30)).collect()), // tx_hash
		Token::Uint(nonce.into()),       // nonce (randomized for uniqueness)
		Token::Uint(500000.into()),      // gas_limit
		Token::Uint(1337.into()),        // slot
	];

	encode(&tokens)
}

#[tokio::test]
async fn test_complete_commitment_flow() {
	let context = create_test_context().await;

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_valid_payload(),
		slasher: context.config.validation.slasher_address.clone(),
	};

	// Generate request hash
	let request_hash = generate_request_hash(&request).unwrap();

	// Check that commitment doesn't exist yet
	let exists = context.database.commitment_exists(&request_hash).await.unwrap();
	assert!(!exists);

	// Create commitment
	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload.clone(),
		request_hash: request_hash.clone(),
		slasher: request.slasher.clone(),
	};

	// Sign the commitment
	let signature = sign_commitment(&commitment, &context.config.signing.private_key).unwrap();

	let signed_commitment = SignedCommitment {
		commitment,
		signature,
	};

	// Save to database
	let save_result = context.database.save_commitment(&signed_commitment).await;
	assert!(save_result.is_ok());

	// Verify commitment structure
	assert_eq!(signed_commitment.commitment.commitment_type, 1);
	assert_eq!(signed_commitment.commitment.payload, request.payload);
	assert_eq!(signed_commitment.commitment.slasher, context.config.validation.slasher_address);
	assert!(!signed_commitment.commitment.request_hash.is_empty());
	assert!(signed_commitment.commitment.request_hash.starts_with("0x"));
	assert_eq!(signed_commitment.commitment.request_hash.len(), 66); // 0x + 64 hex chars

	// Verify signature format
	assert!(!signed_commitment.signature.is_empty());
	assert!(signed_commitment.signature.starts_with("0x"));
	assert_eq!(signed_commitment.signature.len(), 130); // 0x + 128 hex chars (64 bytes)

	// Test retrieval from database
	let retrieved_result = context.database.get_commitment_by_hash(&request_hash).await;
	assert!(retrieved_result.is_ok());

	let retrieved_commitment = retrieved_result.unwrap().unwrap();

	// Verify retrieved commitment matches original
	assert_eq!(retrieved_commitment.commitment.commitment_type, signed_commitment.commitment.commitment_type);
	assert_eq!(retrieved_commitment.commitment.payload, signed_commitment.commitment.payload);
	assert_eq!(retrieved_commitment.commitment.request_hash, signed_commitment.commitment.request_hash);
	assert_eq!(retrieved_commitment.commitment.slasher, signed_commitment.commitment.slasher);
	assert_eq!(retrieved_commitment.signature, signed_commitment.signature);
}

#[tokio::test]
async fn test_duplicate_commitment_prevention() {
	let context = create_test_context().await;

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_valid_payload(),
		slasher: context.config.validation.slasher_address.clone(),
	};

	let request_hash = generate_request_hash(&request).unwrap();

	// Create and save first commitment
	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload.clone(),
		request_hash: request_hash.clone(),
		slasher: request.slasher.clone(),
	};

	let signature = sign_commitment(&commitment, &context.config.signing.private_key).unwrap();
	let signed_commitment = SignedCommitment { commitment, signature };

	// First save should succeed
	let result1 = context.database.save_commitment(&signed_commitment).await;
	assert!(result1.is_ok());

	// Second identical save should fail (duplicate key constraint)
	let result2 = context.database.save_commitment(&signed_commitment).await;
	assert!(result2.is_err());
}

#[tokio::test]
async fn test_commitment_result_not_found() {
	let context = create_test_context().await;

	// Test retrieving non-existent commitment
	let non_existent_hash = "0x1234567890123456789012345678901234567890123456789012345678901234";

	let result = context.database.get_commitment_by_hash(non_existent_hash).await;
	assert!(result.is_ok());
	assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_signature_verification() {
	let context = create_test_context().await;

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_valid_payload(),
		slasher: context.config.validation.slasher_address.clone(),
	};

	let request_hash = generate_request_hash(&request).unwrap();

	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload,
		request_hash,
		slasher: request.slasher,
	};

	let signature = sign_commitment(&commitment, &context.config.signing.private_key).unwrap();

	// Verify signature using crypto module
	let secp = Secp256k1::new();
	let public_key = context.config.signing.private_key.public_key(&secp);

	let is_valid = verify_commitment_signature(&commitment, &signature, &public_key)
		.expect("Failed to verify signature");

	assert!(is_valid, "Signature verification failed");
}

#[tokio::test]
async fn test_multiple_different_commitments() {
	let context = create_test_context().await;

	// Create different payloads for different commitments
	let payloads = vec![
		create_payload_with_slot(1000),
		create_payload_with_slot(2000),
		create_payload_with_slot(3000),
	];

	let mut commitment_hashes = Vec::new();

	// Create multiple commitments
	for payload in payloads {
		let request = CommitmentRequest {
			commitment_type: 1,
			payload: payload.clone(),
			slasher: context.config.validation.slasher_address.clone(),
		};

		let request_hash = generate_request_hash(&request).unwrap();

		let commitment = Commitment {
			commitment_type: request.commitment_type,
			payload,
			request_hash: request_hash.clone(),
			slasher: request.slasher,
		};

		let signature = sign_commitment(&commitment, &context.config.signing.private_key).unwrap();
		let signed_commitment = SignedCommitment { commitment, signature };

		let result = context.database.save_commitment(&signed_commitment).await;
		assert!(result.is_ok());

		commitment_hashes.push(request_hash);
	}

	// Verify all commitments can be retrieved
	for hash in commitment_hashes {
		let result = context.database.get_commitment_by_hash(&hash).await;
		assert!(result.is_ok());
		assert!(result.unwrap().is_some());
	}
}

/// Helper function to create payload with specific slot
fn create_payload_with_slot(slot: u64) -> Vec<u8> {
	let _types = vec![
		ParamType::FixedBytes(32), // tx_hash (Bytes32)
		ParamType::Uint(256),      // nonce (uint256)
		ParamType::Uint(256),      // gas_limit (uint256)
		ParamType::Uint(64),       // slot (uint64)
	];

	let tokens = vec![
		Token::FixedBytes(vec![0x9f, 0xbb].into_iter().chain(std::iter::repeat(0).take(30)).collect()), // tx_hash
		Token::Uint(9.into()),           // nonce
		Token::Uint(500000.into()),      // gas_limit
		Token::Uint(slot.into()),        // slot
	];

	encode(&tokens)
}

#[tokio::test]
async fn test_database_operations_directly() {
	let pool = setup_test_pool().await;
	let db_context = DatabaseContext::new(pool);

	// Create test commitment
	let commitment = Commitment {
		commitment_type: 1,
		payload: create_valid_payload(),
		request_hash: "0x1234567890123456789012345678901234567890123456789012345678901234".to_string(),
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};

	let signed_commitment = SignedCommitment {
		commitment,
		signature: "0x12345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678".to_string(),
	};

	// Test save
	let save_result = db_context.save_commitment(&signed_commitment).await;
	assert!(save_result.is_ok());

	// Test existence check
	let exists_result = db_context.commitment_exists(&signed_commitment.commitment.request_hash).await;
	assert!(exists_result.is_ok());
	assert!(exists_result.unwrap());

	// Test retrieval
	let get_result = db_context.get_commitment_by_hash(&signed_commitment.commitment.request_hash).await;
	assert!(get_result.is_ok());

	let retrieved = get_result.unwrap();
	assert!(retrieved.is_some());

	let retrieved_commitment = retrieved.unwrap();
	assert_eq!(retrieved_commitment.commitment.commitment_type, signed_commitment.commitment.commitment_type);
	assert_eq!(retrieved_commitment.commitment.payload, signed_commitment.commitment.payload);
	assert_eq!(retrieved_commitment.commitment.request_hash, signed_commitment.commitment.request_hash);
	assert_eq!(retrieved_commitment.commitment.slasher, signed_commitment.commitment.slasher);
	assert_eq!(retrieved_commitment.signature, signed_commitment.signature);
}