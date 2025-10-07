//! Integration tests for signing and database operations
//!
//! These tests verify the complete functionality of the commitment system:
//! - ECDSA signing of commitments
//! - Database storage and retrieval
//! - End-to-end functionality
//!
//! ## Test Isolation
//!
//! Each test runs in its own isolated PostgreSQL database to prevent cross-test
//! race conditions. The `TestFixture` creates a unique database per test using
//! process ID and UUID for the database name (e.g., `test_12345_abc123...`).
//!
//! This isolation strategy:
//! - Eliminates race conditions from concurrent test execution
//! - Allows tests to run in parallel with --test-threads=N
//! - Prevents DELETE FROM queries from affecting other running tests
//! - Ensures each test starts with a clean schema
//!
//! Test databases are automatically cleaned up on test completion. If cleanup
//! fails, run: `./scripts/cleanup_test_dbs.sh`

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
use uuid::Uuid;

/// Test fixture that provides isolated database per test using unique database names
///
/// Each test gets its own database to prevent cross-test race conditions.
/// Test databases are named with process ID and UUID for uniqueness.
///
/// Note: Test databases are automatically cleaned up on drop via async task.
/// If cleanup fails, you can manually remove them with:
/// ```sql
/// SELECT 'DROP DATABASE "' || datname || '";'
/// FROM pg_database WHERE datname LIKE 'test_%';
/// ```
struct TestFixture {
	pool: PgPool,
	db_name: String,
	admin_pool: PgPool,
}

impl TestFixture {
	/// Create a new test fixture with a unique isolated database
	async fn new() -> Self {
		let base_url = std::env::var("TEST_DATABASE_URL")
			.unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/postgres".to_string());

		// Generate unique database name for this test
		let db_name = format!("test_{}_{}",
			std::process::id(),
			Uuid::new_v4().to_string().replace('-', "_")
		);

		// Connect to postgres database to create test database
		let admin_pool = sqlx::postgres::PgPoolOptions::new()
			.max_connections(1)
			.connect(&base_url)
			.await
			.expect("Failed to connect to admin database");

		// Create the unique test database
		let create_db_query = format!("CREATE DATABASE \"{}\"", db_name);
		sqlx::query(&create_db_query)
			.execute(&admin_pool)
			.await
			.expect("Failed to create test database");

		// Build connection string for the test database, preserving query string
		let test_db_url = {
			// Split URL into base path and query/fragment
			let (path_part, query_part) = if let Some(idx) = base_url.find('?') {
				(&base_url[..idx], &base_url[idx..])
			} else {
				(base_url.as_str(), "")
			};

			// Replace database name in path while preserving query string
			let base_path = path_part.rsplit_once('/').unwrap().0;
			format!("{}/{}{}", base_path, db_name, query_part)
		};

		// Connect to the test database
		let pool = sqlx::postgres::PgPoolOptions::new()
			.max_connections(5)
			.connect(&test_db_url)
			.await
			.expect("Failed to connect to test database");

		// Run migrations on the test database
		sqlx::migrate!("./migrations")
			.run(&pool)
			.await
			.expect("Failed to run migrations on test database");

		Self { pool, db_name, admin_pool }
	}

	/// Get the pool for creating DatabaseContext
	fn pool(&self) -> PgPool {
		self.pool.clone()
	}
}

impl Drop for TestFixture {
	fn drop(&mut self) {
		// Clean up test database - use Handle::current() from existing runtime
		let db_name = self.db_name.clone();
		let pool = self.pool.clone();
		let admin_pool = self.admin_pool.clone();

		// Block on cleanup using the current runtime handle
		if let Ok(handle) = tokio::runtime::Handle::try_current() {
			handle.spawn(async move {
				// Close the pool first
				pool.close().await;

				// Force disconnect all connections
				let force_disconnect = format!(
					"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}'",
					db_name
				);
				let _ = sqlx::query(&force_disconnect).execute(&admin_pool).await;

				// Small delay to ensure connections are terminated
				tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

				// Drop the test database
				let drop_db_query = format!("DROP DATABASE IF EXISTS \"{}\"", db_name);
				let _ = sqlx::query(&drop_db_query).execute(&admin_pool).await;
			});
		}
	}
}

/// Setup test database pool for integration tests (legacy - uses shared database)
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

	pool
}

/// Create test RPC context with a specific database pool
async fn create_test_context_with_pool(pool: PgPool) -> Arc<RpcContext> {
	let db_context = DatabaseContext::new(pool);

	// Load config (uses TEST environment variables if available)
	// If loading fails due to beacon endpoint validation, use default with test endpoint
	let mut config = match Config::load() {
		Ok(c) => c,
		Err(_) => {
			let mut c = Config::default();
			// Set a valid test beacon endpoint
			c.beacon_api.primary_endpoint = "http://localhost:5051".to_string();
			c
		}
	};

	// Ensure beacon endpoint is valid for testing
	if config.beacon_api.primary_endpoint.contains("${BEACON_API_ENDPOINT}") ||
	   config.beacon_api.primary_endpoint.contains("YOUR_API_KEY") {
		config.beacon_api.primary_endpoint = "http://localhost:5051".to_string();
	}

	// Create fee engine for testing
	use preconfirmation_gateway::api::reth::{RethApiClient, RethApiConfig};
	use preconfirmation_gateway::services::fee_pricing::FeePricingEngine;

	let reth_client = Arc::new(
		RethApiClient::new(RethApiConfig::default()).unwrap()
	);
	let database_arc = Arc::new(db_context.clone());
	let config_arc = Arc::new(config.clone());
	let fee_engine = Arc::new(FeePricingEngine::new(reth_client, database_arc, config_arc.clone()));

	// Create beacon API client for testing
	use preconfirmation_gateway::api::beacon::BeaconApiClient;
	let beacon_client = Arc::new(
		BeaconApiClient::new(config.beacon_api.clone()).unwrap()
	);

	Arc::new(RpcContext::new(db_context, config, fee_engine, beacon_client))
}

/// Create test RPC context with database and signing configuration (legacy - uses shared DB)
async fn create_test_context() -> Arc<RpcContext> {
	let pool = setup_test_pool().await;
	create_test_context_with_pool(pool).await
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
	let fixture = TestFixture::new().await;
	let context = create_test_context_with_pool(fixture.pool()).await;

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
	let signature = sign_commitment(&commitment, &context.config.signing.ecdsa_private_key).unwrap();

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
	let fixture = TestFixture::new().await;
	let context = create_test_context_with_pool(fixture.pool()).await;

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

	let signature = sign_commitment(&commitment, &context.config.signing.ecdsa_private_key).unwrap();
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
	let fixture = TestFixture::new().await;
	let context = create_test_context_with_pool(fixture.pool()).await;

	// Test retrieving non-existent commitment
	let non_existent_hash = "0x1234567890123456789012345678901234567890123456789012345678901234";

	let result = context.database.get_commitment_by_hash(non_existent_hash).await;
	assert!(result.is_ok());
	assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_signature_verification() {
	let fixture = TestFixture::new().await;
	let context = create_test_context_with_pool(fixture.pool()).await;

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

	let signature = sign_commitment(&commitment, &context.config.signing.ecdsa_private_key).unwrap();

	// Verify signature using crypto module
	let secp = Secp256k1::new();
	let public_key = context.config.signing.ecdsa_private_key.public_key(&secp);

	let is_valid = verify_commitment_signature(&commitment, &signature, &public_key)
		.expect("Failed to verify signature");

	assert!(is_valid, "Signature verification failed");
}

#[tokio::test]
async fn test_multiple_different_commitments() {
	let fixture = TestFixture::new().await;
	let context = create_test_context_with_pool(fixture.pool()).await;

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

		let signature = sign_commitment(&commitment, &context.config.signing.ecdsa_private_key).unwrap();
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
	let fixture = TestFixture::new().await;
	let db_context = DatabaseContext::new(fixture.pool());

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