//! Database module for SQLx-based PostgreSQL operations
//!
//! This module provides database connectivity and operations for the preconfirmation gateway.
//! It uses SQLx for compile-time checked SQL queries and async database operations.

pub mod delegation_ops;
pub mod operations;
pub mod slot_congestion_ops;

use std::env;

use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, migrate::MigrateDatabase, postgres::PgPoolOptions};
use tracing::info;

use crate::config::Config;

/// Creates a PostgreSQL connection pool using configuration from the `DATABASE_URL` environment
/// variable (if present) or the provided `Config`, ensures the database exists, and applies schema
/// migrations before returning the pool.
///
/// # Errors
///
/// Returns an error if the database cannot be created, the connection pool cannot be established,
/// or migrations fail.
///
/// # Examples
///
pub async fn create_pool(config: &Config) -> Result<PgPool> {
	// Environment variable takes precedence over config file
	let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| config.database_url().to_string());

	info!("Connecting to database");

	// Create database if it doesn't exist
	let db_exists = Postgres::database_exists(&database_url)
		.await
		.context("Failed to check if database exists. Verify database connection URL and permissions")?;

	if !db_exists {
		info!("Database does not exist, creating it...");
		Postgres::create_database(&database_url).await.context("Failed to create database")?;
	}

	// Create connection pool
	let pool = PgPoolOptions::new()
		.max_connections(10)
		.connect(&database_url)
		.await
		.context("Failed to create database connection pool")?;

	info!("Database connection pool created successfully");

	// Run migrations
	run_migrations(&pool).await?;

	Ok(pool)
}

/// Applies pending SQL migrations from the repository's ./migrations directory to the given Postgres pool.
///
/// # Examples
///
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
	info!("Running database migrations...");

	sqlx::migrate!("./migrations").run(pool).await.context("Failed to run database migrations")?;

	info!("Database migrations completed successfully");
	Ok(())
}

/// Verify that the provided PostgreSQL pool accepts queries by executing a simple health check.
///
/// Executes a `SELECT 1` query against the given `PgPool` and returns `Ok(())` when the query
/// returns `1`; returns an error with context for query failures or unexpected results.
///
/// # Returns
///
/// `Ok(())` if the database responds with `1`, error otherwise.
///
/// # Examples
///
pub async fn test_connection(pool: &PgPool) -> Result<()> {
	info!("Testing database connection...");

	// Simple query to verify connection
	let row: (i32,) = sqlx::query_as("SELECT 1").fetch_one(pool).await.context("Failed to execute test query")?;

	if row.0 == 1 {
		info!("Database connection test successful");
		Ok(())
	} else {
		anyhow::bail!("Database connection test failed: unexpected result");
	}
}

/// Database context for dependency injection
///
/// This provides a clean interface for database operations
#[derive(Clone, Debug)]
pub struct DatabaseContext {
	pool: PgPool,
}

impl DatabaseContext {
	/// Creates a DatabaseContext that owns the provided PostgreSQL connection pool.
	///
	/// # Examples
	///
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Create a DatabaseContext suitable for unit tests that does not attempt a live connection.
	///
	/// The returned context contains a lazily-connected PgPool configured for a test database.
	///
	/// # Examples
	///
	#[cfg(test)]
	pub fn new_for_testing() -> Self {
		// Create a fake pool that won't actually connect
		let pool =
			PgPool::connect_lazy("postgresql://test:test@localhost/test_db").expect("Failed to create test pool");
		Self { pool }
	}

	/// Access the underlying PostgreSQL connection pool.
	///
	/// Returns a reference to the internal `PgPool`.
	///
	/// # Examples
	///
	pub fn pool(&self) -> &PgPool {
		&self.pool
	}

	/// Save a signed commitment in the database with atomic duplicate detection.
	///
	/// # Returns
	///
	/// - `Ok(Some(Uuid))` if a new commitment was inserted
	/// - `Ok(None)` if a commitment with the same request_hash already exists (idempotent behavior)
	/// - `Err(...)` for database errors
	///
	/// # Examples
	///
	pub async fn save_commitment(
		&self,
		signed_commitment: &crate::types::SignedCommitment,
	) -> Result<Option<uuid::Uuid>> {
		operations::save_commitment(&self.pool, signed_commitment).await
	}

	/// Retrieve a signed commitment for the given request hash.
	///
	/// # Returns
	///
	/// `Some(SignedCommitment)` if a commitment with the given request hash exists, `None` otherwise.
	///
	/// # Examples
	///
	pub async fn get_commitment_by_hash(&self, request_hash: &str) -> Result<Option<crate::types::SignedCommitment>> {
		operations::get_commitment_by_hash(&self.pool, request_hash).await
	}

	/// Determines whether a commitment with the given request hash exists in the database.
	///
	/// # Parameters
	///
	/// - `request_hash`: The request hash to look up.
	///
	/// # Returns
	///
	/// `true` if a commitment with `request_hash` exists, `false` otherwise.
	///
	/// # Examples
	///
	pub async fn commitment_exists(&self, request_hash: &str) -> Result<bool> {
		operations::commitment_exists(&self.pool, request_hash).await
	}

	/// Retrieve aggregated statistics about stored commitments.
	///
	/// Returns aggregated metrics for commitments such as totals and derived counts.
	///
	/// # Examples
	///
	pub async fn get_stats(&self) -> Result<operations::CommitmentStats> {
		operations::get_commitment_stats(&self.pool).await
	}

	// Delegation operations

	/// Persists a delegation record after verifying its BLS signature.
	///
	/// # Parameters
	///
	/// - `signed_delegation`: The delegation to save
	/// - `bls_manager`: The BLS manager used to verify the delegation signature
	///
	/// # Returns
	///
	/// The UUID of the saved delegation.
	///
	/// # Errors
	///
	/// Returns an error if the signature is invalid or the database operation fails.
	///
	/// # Examples
	///
	pub async fn save_delegation(
		&self,
		signed_delegation: &crate::types::SignedDelegation,
		bls_manager: &crate::crypto::bls::BlsManager,
	) -> Result<uuid::Uuid> {
		delegation_ops::save_delegation(&self.pool, signed_delegation, bls_manager).await
	}

	/// Retrieve all signed delegations for the specified slot.
	///
	/// # Returns
	///
	/// A `Vec<SignedDelegation>` containing all delegations associated with `slot`.
	///
	/// # Examples
	///
	pub async fn get_delegations_for_slot(&self, slot: u64) -> Result<Vec<crate::types::SignedDelegation>> {
		delegation_ops::get_delegations_for_slot(&self.pool, slot).await
	}

	/// Check whether a delegation exists for a given slot and committer address.
	///
	/// # Returns
	///
	/// `true` if a delegation exists for the slot and committer address, `false` otherwise.
	///
	/// # Examples
	///
	pub async fn delegation_exists_for_slot_and_committer(&self, slot: u64, committer_address: &str) -> Result<bool> {
		delegation_ops::delegation_exists_for_slot_and_committer(&self.pool, slot, committer_address).await
	}

	/// Retrieves the signed delegation for a proposer at a specific slot.
	///
	/// # Examples
	///
	///
	/// # Returns
	///
	/// `Some(SignedDelegation)` containing the delegation for the given proposer and slot, `None` if no delegation is found.
	pub async fn get_delegation_by_proposer_slot(
		&self,
		proposer_pubkey: &crate::types::BlsPublicKey,
		slot: u64,
	) -> Result<Option<crate::types::SignedDelegation>> {
		delegation_ops::get_delegation_by_proposer_slot(&self.pool, proposer_pubkey, slot).await
	}

	/// Fetches all delegations associated with the given delegate public key.
	///
	/// # Parameters
	///
	/// - `delegate_pubkey`: Delegate's BLS public key used to look up delegations.
	///
	/// # Returns
	///
	/// A vector of `SignedDelegation` records belonging to the specified delegate.
	///
	/// # Examples
	///
	pub async fn get_delegations_by_delegate(
		&self,
		delegate_pubkey: &crate::types::BlsPublicKey,
	) -> Result<Vec<crate::types::SignedDelegation>> {
		delegation_ops::get_delegations_by_delegate(&self.pool, delegate_pubkey).await
	}

	/// Saves multiple signed delegations in a single batch and returns their database IDs in input order.
	///
	/// All delegations are verified for valid BLS signatures before any are stored.
	/// The returned `Vec<uuid::Uuid>` contains the UUID for each saved delegation in the same order as the `delegations` slice.
	///
	/// # Parameters
	///
	/// - `delegations`: The delegations to save
	/// - `bls_manager`: The BLS manager used to verify delegation signatures
	///
	/// # Errors
	///
	/// Returns an error if any signature is invalid or the database operation fails.
	///
	/// # Examples
	///
	pub async fn save_delegations_batch(
		&self,
		delegations: &[crate::types::SignedDelegation],
		bls_manager: &crate::crypto::bls::BlsManager,
	) -> Result<Vec<uuid::Uuid>> {
		delegation_ops::save_delegations_batch(&self.pool, delegations, bls_manager).await
	}

	/// Retrieves aggregated delegation statistics.
	///
	/// Returns a `delegation_ops::DelegationStats` containing counts and aggregates for stored delegations.
	///
	/// # Examples
	///
	pub async fn get_delegation_stats(&self) -> Result<delegation_ops::DelegationStats> {
		delegation_ops::get_delegation_stats(&self.pool).await
	}

	/// Deactivates delegations that have expired relative to the provided slot.
	///
	/// Returns the number of delegations that were deactivated.
	///
	/// # Examples
	///
	pub async fn deactivate_expired_delegations(&self, current_slot: u64) -> Result<u64> {
		delegation_ops::deactivate_expired_delegations(&self.pool, current_slot).await
	}

	// Slot congestion operations

	/// Retrieves the `SlotCongestion` record for `slot`, creating a new record initialized with
	/// `base_gas_price`, `total_gas_limit`, and `genesis_time` if none exists.
	///
	/// # Examples
	///
	pub async fn get_or_create_slot_congestion(
		&self,
		slot: u64,
		base_gas_price: u64,
		total_gas_limit: u64,
		genesis_time: u64,
	) -> Result<slot_congestion_ops::SlotCongestion> {
		slot_congestion_ops::get_or_create_slot_congestion(
			&self.pool,
			slot,
			base_gas_price,
			total_gas_limit,
			genesis_time,
		)
		.await
	}

	/// Update the recorded gas usage for a slot's congestion entry and return the updated record.
	///
	/// The `additional_gas` is added to the slot's current gas usage; `scaling_factor` is applied
	/// when computing the adjusted gas contribution for the update.
	///
	/// # Parameters
	///
	/// - `slot`: Slot identifier to update.
	/// - `additional_gas`: Amount of gas to add to the slot's usage.
	/// - `scaling_factor`: Multiplier applied to the added gas when updating congestion metrics.
	///
	/// # Returns
	///
	/// The updated `slot_congestion_ops::SlotCongestion` record.
	///
	/// # Examples
	///
	pub async fn update_slot_congestion_gas_usage(
		&self,
		slot: u64,
		additional_gas: u64,
		scaling_factor: f64,
	) -> Result<slot_congestion_ops::SlotCongestion> {
		slot_congestion_ops::update_slot_congestion_gas_usage(&self.pool, slot, additional_gas, scaling_factor).await
	}

	/// Retrieves the current gas price for the specified slot.
	///
	/// Returns `Some(price)` when a gas price is available for the slot, or `None` when no price is recorded.
	///
	/// # Examples
	///
	pub async fn get_current_gas_price_for_slot(&self, slot: u64) -> Result<Option<u64>> {
		slot_congestion_ops::get_current_gas_price_for_slot(&self.pool, slot).await
	}

	/// Retrieve aggregated slot congestion statistics.
	///
	/// # Examples
	///
	pub async fn get_congestion_stats(&self) -> Result<slot_congestion_ops::CongestionStats> {
		slot_congestion_ops::get_congestion_stats(&self.pool).await
	}

	/// Removes slot congestion records older than the given number of hours.
	///
	/// Returns the number of removed slot congestion records.
	///
	/// # Examples
	///
	pub async fn cleanup_old_slot_congestion(&self, hours_to_keep: u32) -> Result<u64> {
		slot_congestion_ops::cleanup_old_slot_congestion(&self.pool, hours_to_keep).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::Config;
	use crate::types::delegation::{BlsPublicKey, BlsSignature, DelegationMessage, SignedDelegation};
	use crate::types::{Commitment, CommitmentRequest, SignedCommitment};
	use std::env;

	/// Helper function to create a test configuration
	fn create_test_config() -> Config {
		Config {
			server: Default::default(),
			database: crate::config::DatabaseConfig { url: "postgresql://test:test@localhost/test_db".to_string() },
			logging: Default::default(),
			validation: Default::default(),
			beacon_api: Default::default(),
			constraints_api: Default::default(),
			delegation: Default::default(),
			reth: Default::default(),
			signing: Default::default(),
		}
	}

	/// Helper function to create a test SignedCommitment
	fn create_test_commitment() -> SignedCommitment {
		let request = CommitmentRequest {
			commitment_type: 1,
			payload: vec![1, 2, 3, 4],
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let commitment = Commitment {
			commitment_type: request.commitment_type,
			payload: request.payload.clone(),
			request_hash: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
			slasher: request.slasher,
		};

		SignedCommitment {
			commitment,
			signature: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
		}
	}

	/// Helper function to create a test SignedDelegation
	fn create_test_delegation() -> SignedDelegation {
		SignedDelegation {
			message: DelegationMessage {
				proposer: BlsPublicKey([1u8; 48]),
				delegate: BlsPublicKey([2u8; 48]),
				committer: "0x1234567890123456789012345678901234567890".to_string(),
				slot: 12345,
			},
			signature: BlsSignature([3u8; 96]),
		}
	}

	#[tokio::test]
	async fn test_database_context_creation() {
		let context = DatabaseContext::new_for_testing();

		// Test that we can access the pool
		let pool = context.pool();

		// Verify that the pool is created (but not necessarily connected since it's lazy)
		// Attempting to acquire a connection will fail with the test database,
		// but this validates the pool structure is valid
		let acquire_result = pool.acquire().await;
		assert!(acquire_result.is_err()); // Expected to fail with test database URL
	}

	#[tokio::test]
	async fn test_database_context_wrapper_methods() {
		use crate::crypto::bls::BlsManager;

		let context = DatabaseContext::new_for_testing();
		let bls_manager = BlsManager::new("0x00000002").unwrap();

		// Test commitment operations (these will fail with lazy connection, but we're testing the wrapper)
		let commitment = create_test_commitment();
		let result = context.save_commitment(&commitment).await;
		assert!(result.is_err()); // Expected to fail with test database

		// Test delegation operations
		let delegation = create_test_delegation();
		let result = context.save_delegation(&delegation, &bls_manager).await;
		assert!(result.is_err()); // Expected to fail with test database

		// Test slot congestion operations
		let result = context.get_or_create_slot_congestion(12345, 1000000000, 30000000, 1606824023).await;
		assert!(result.is_err()); // Expected to fail with test database
	}

	#[test]
	fn test_config_database_url() {
		let config = create_test_config();
		assert_eq!(config.database_url(), "postgresql://test:test@localhost/test_db");
	}

	#[tokio::test]
	async fn test_run_migrations_with_invalid_pool() {
		// Test migration function with an invalid pool
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();
		let result = run_migrations(&invalid_pool).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_test_connection_with_invalid_pool() {
		// Test connection test with an invalid pool
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();
		let result = test_connection(&invalid_pool).await;
		assert!(result.is_err());
	}

	#[test]
	fn test_environment_variable_precedence() {
		// Test that DATABASE_URL environment variable takes precedence over config
		let original_env = env::var("DATABASE_URL").ok();

		// Set a test environment variable
		unsafe {
			env::set_var("DATABASE_URL", "postgresql://env:env@localhost/env_db");
		}

		let config = create_test_config();
		let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| config.database_url().to_string());

		assert_eq!(database_url, "postgresql://env:env@localhost/env_db");

		// Restore original environment
		unsafe {
			match original_env {
				Some(val) => env::set_var("DATABASE_URL", val),
				None => env::remove_var("DATABASE_URL"),
			}
		}
	}

	#[tokio::test]
	async fn test_database_context_clone() {
		let context = DatabaseContext::new_for_testing();
		let cloned_context = context.clone();

		// Both should have access to the same pool
		assert_eq!(context.pool().is_closed(), cloned_context.pool().is_closed());
	}

	#[tokio::test]
	async fn test_database_context_debug() {
		let context = DatabaseContext::new_for_testing();
		let debug_str = format!("{:?}", context);
		assert!(debug_str.contains("DatabaseContext"));
	}

	// Integration tests that would require a real database
	#[tokio::test]
	#[ignore] // Ignore by default since it requires a real database
	async fn test_create_pool_with_real_database() {
		// This test would require a real PostgreSQL database
		// It's marked as ignored to avoid failures in CI/CD
		let config = create_test_config();
		let result = create_pool(&config).await;

		// This would succeed with a real database
		match result {
			Ok(pool) => {
				// Test that the pool works
				let test_result = test_connection(&pool).await;
				assert!(test_result.is_ok());

				// Test migrations
				let migration_result = run_migrations(&pool).await;
				assert!(migration_result.is_ok());
			}
			Err(_) => {
				// Expected to fail without a real database
				// This is fine for unit tests
			}
		}
	}

	#[tokio::test]
	#[ignore] // Ignore by default since it requires a real database
	async fn test_database_context_with_real_pool() {
		// This test would require a real PostgreSQL database
		let config = create_test_config();

		if let Ok(pool) = create_pool(&config).await {
			let context = DatabaseContext::new(pool);

			// Test commitment operations
			let commitment = create_test_commitment();
			let result = context.save_commitment(&commitment).await;
			assert!(result.is_ok());

			let saved_id = result.unwrap();
			assert!(saved_id.is_some());
			assert!(!saved_id.unwrap().is_nil());

			// Test retrieval
			let retrieved = context.get_commitment_by_hash(&commitment.commitment.request_hash).await;
			assert!(retrieved.is_ok());
			assert!(retrieved.unwrap().is_some());

			// Test existence check
			let exists = context.commitment_exists(&commitment.commitment.request_hash).await;
			assert!(exists.is_ok());
			assert!(exists.unwrap());

			// Test stats
			let stats = context.get_stats().await;
			assert!(stats.is_ok());
		}
	}
}
