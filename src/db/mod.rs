//! Database module for SQLx-based PostgreSQL operations
//!
//! This module provides database connectivity and operations for the preconfirmation gateway.
//! It uses SQLx for compile-time checked SQL queries and async database operations.

pub mod delegation_ops;
pub mod operations;
pub mod slot_congestion_ops;

use std::env;

use anyhow::{Context, Result};
use sqlx::{migrate::MigrateDatabase, postgres::PgPoolOptions, PgPool, Postgres};
use tracing::info;

use crate::config::Config;

/// Create a PostgreSQL connection pool using SQLx
///
/// This replaces the deadpool-postgres functionality with SQLx
pub async fn create_pool(config: &Config) -> Result<PgPool> {
	// Environment variable takes precedence over config file
	let database_url = env::var("DATABASE_URL")
		.unwrap_or_else(|_| config.database_url().to_string());

	info!("Connecting to database");

	// Create database if it doesn't exist
	if !Postgres::database_exists(&database_url).await.unwrap_or(false) {
		info!("Database does not exist, creating it...");
		Postgres::create_database(&database_url)
			.await
			.context("Failed to create database")?;
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

/// Run database migrations
///
/// This ensures the database schema is up to date
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
	info!("Running database migrations...");

	sqlx::migrate!("./migrations")
		.run(pool)
		.await
		.context("Failed to run database migrations")?;

	info!("Database migrations completed successfully");
	Ok(())
}

/// Test database connection
///
/// This verifies that the database connection is working
pub async fn test_connection(pool: &PgPool) -> Result<()> {
	info!("Testing database connection...");

	// Simple query to verify connection
	let row: (i32,) = sqlx::query_as("SELECT 1")
		.fetch_one(pool)
		.await
		.context("Failed to execute test query")?;

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
	/// Create new database context
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Create a database context for testing (without actual connection)
	#[cfg(test)]
	pub fn new_for_testing() -> Self {
		// Create a fake pool that won't actually connect
		let pool = PgPool::connect_lazy("postgresql://test:test@localhost/test_db")
			.expect("Failed to create test pool");
		Self { pool }
	}

	/// Get reference to the connection pool
	pub fn pool(&self) -> &PgPool {
		&self.pool
	}

	/// Save a commitment to the database
	pub async fn save_commitment(
		&self,
		signed_commitment: &crate::types::SignedCommitment,
	) -> Result<uuid::Uuid> {
		operations::save_commitment(&self.pool, signed_commitment).await
	}

	/// Get a commitment by request hash
	pub async fn get_commitment_by_hash(
		&self,
		request_hash: &str,
	) -> Result<Option<crate::types::SignedCommitment>> {
		operations::get_commitment_by_hash(&self.pool, request_hash).await
	}

	/// Check if a commitment exists
	pub async fn commitment_exists(&self, request_hash: &str) -> Result<bool> {
		operations::commitment_exists(&self.pool, request_hash).await
	}

	/// Get commitment statistics
	pub async fn get_stats(&self) -> Result<operations::CommitmentStats> {
		operations::get_commitment_stats(&self.pool).await
	}

	// Delegation operations

	/// Save a delegation to the database
	pub async fn save_delegation(
		&self,
		signed_delegation: &crate::types::SignedDelegation,
	) -> Result<uuid::Uuid> {
		delegation_ops::save_delegation(&self.pool, signed_delegation).await
	}

	/// Get delegations for a specific slot
	pub async fn get_delegations_for_slot(
		&self,
		slot: u64,
	) -> Result<Vec<crate::types::SignedDelegation>> {
		delegation_ops::get_delegations_for_slot(&self.pool, slot).await
	}

	/// Check if delegation exists for slot and committer
	pub async fn delegation_exists_for_slot_and_committer(
		&self,
		slot: u64,
		committer_address: &str,
	) -> Result<bool> {
		delegation_ops::delegation_exists_for_slot_and_committer(&self.pool, slot, committer_address).await
	}

	/// Get delegation by proposer and slot
	pub async fn get_delegation_by_proposer_slot(
		&self,
		proposer_pubkey: &crate::types::BlsPublicKey,
		slot: u64,
	) -> Result<Option<crate::types::SignedDelegation>> {
		delegation_ops::get_delegation_by_proposer_slot(&self.pool, proposer_pubkey, slot).await
	}

	/// Get delegations by delegate pubkey
	pub async fn get_delegations_by_delegate(
		&self,
		delegate_pubkey: &crate::types::BlsPublicKey,
	) -> Result<Vec<crate::types::SignedDelegation>> {
		delegation_ops::get_delegations_by_delegate(&self.pool, delegate_pubkey).await
	}

	/// Batch save delegations
	pub async fn save_delegations_batch(
		&self,
		delegations: &[crate::types::SignedDelegation],
	) -> Result<Vec<uuid::Uuid>> {
		delegation_ops::save_delegations_batch(&self.pool, delegations).await
	}

	/// Get delegation statistics
	pub async fn get_delegation_stats(&self) -> Result<delegation_ops::DelegationStats> {
		delegation_ops::get_delegation_stats(&self.pool).await
	}

	/// Deactivate expired delegations
	pub async fn deactivate_expired_delegations(&self, current_slot: u64) -> Result<u64> {
		delegation_ops::deactivate_expired_delegations(&self.pool, current_slot).await
	}

	// Slot congestion operations

	/// Get or create slot congestion tracking record
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
		).await
	}

	/// Update slot congestion with additional gas usage
	pub async fn update_slot_congestion_gas_usage(
		&self,
		slot: u64,
		additional_gas: u64,
		scaling_factor: f64,
	) -> Result<slot_congestion_ops::SlotCongestion> {
		slot_congestion_ops::update_slot_congestion_gas_usage(
			&self.pool,
			slot,
			additional_gas,
			scaling_factor,
		).await
	}

	/// Get current gas price for a slot
	pub async fn get_current_gas_price_for_slot(&self, slot: u64) -> Result<Option<u64>> {
		slot_congestion_ops::get_current_gas_price_for_slot(&self.pool, slot).await
	}

	/// Get congestion statistics
	pub async fn get_congestion_stats(&self) -> Result<slot_congestion_ops::CongestionStats> {
		slot_congestion_ops::get_congestion_stats(&self.pool).await
	}

	/// Cleanup old slot congestion records
	pub async fn cleanup_old_slot_congestion(&self, hours_to_keep: u32) -> Result<u64> {
		slot_congestion_ops::cleanup_old_slot_congestion(&self.pool, hours_to_keep).await
	}
}
