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
/// ```
/// # tokio_test::block_on(async {
/// let cfg = crate::config::Config::default(); // construct appropriate config
/// let pool = crate::db::create_pool(&cfg).await.expect("create pool");
/// // use `pool` for queries...
/// # });
/// ```
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
/// ```no_run
/// use sqlx::PgPool;
///
/// async fn example(pool: &PgPool) -> anyhow::Result<()> {
///     run_migrations(pool).await?;
///     Ok(())
/// }
/// ```
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
/// ```no_run
/// # use sqlx::PgPool;
/// # async fn example(pool: &PgPool) -> anyhow::Result<()> {
/// test_connection(pool).await?;
/// # Ok(())
/// # }
/// ```
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
	/// ```
	/// let ctx = DatabaseContext::new(pool);
	/// ```
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Create a DatabaseContext suitable for unit tests that does not attempt a live connection.
	///
	/// The returned context contains a lazily-connected PgPool configured for a test database.
	///
	/// # Examples
	///
	/// ```
	/// let ctx = crate::db::DatabaseContext::new_for_testing();
	/// // use `ctx` in tests without requiring a real database connection
	/// ```
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
	/// ```no_run
	/// # use crate::db::DatabaseContext;
	/// # use sqlx::PgPool;
	/// # async fn example(ctx: &DatabaseContext) {
	/// let pool: &PgPool = ctx.pool();
	/// # }
	/// ```
	pub fn pool(&self) -> &PgPool {
		&self.pool
	}

	/// Save a signed commitment in the database.
	///
	/// # Returns
	///
	/// The `Uuid` assigned to the saved commitment.
	///
	/// # Examples
	///
	/// ```no_run
	/// # use uuid::Uuid;
	/// # async fn example(db: &crate::db::DatabaseContext, commitment: crate::types::SignedCommitment) -> Result<(), Box<dyn std::error::Error>> {
	/// let id: Uuid = db.save_commitment(&commitment).await?;
	/// println!("saved commitment id = {}", id);
	/// # Ok(())
	/// # }
	/// ```
	pub async fn save_commitment(&self, signed_commitment: &crate::types::SignedCommitment) -> Result<uuid::Uuid> {
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
	/// ```
	/// # async fn example(db: &crate::db::DatabaseContext) -> Result<(), Box<dyn std::error::Error>> {
	/// let maybe_commitment = db.get_commitment_by_hash("request_hash_hex").await?;
	/// if let Some(commitment) = maybe_commitment {
	///     // use `commitment`
	///     println!("{}", commitment.request_hash);
	/// }
	/// # Ok(())
	/// # }
	/// ```
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
	/// ```
	/// # async {
	/// // `ctx` is a `DatabaseContext` with a connected `PgPool`.
	/// let exists = ctx.commitment_exists("some-request-hash").await.unwrap();
	/// if exists {
	///     println!("Commitment found");
	/// } else {
	///     println!("Commitment not found");
	/// }
	/// # };
	/// ```
	pub async fn commitment_exists(&self, request_hash: &str) -> Result<bool> {
		operations::commitment_exists(&self.pool, request_hash).await
	}

	/// Retrieve aggregated statistics about stored commitments.
	///
	/// Returns aggregated metrics for commitments such as totals and derived counts.
	///
	/// # Examples
	///
	/// ```no_run
	/// # use crate::db::DatabaseContext;
	/// # async fn example(ctx: &DatabaseContext) -> anyhow::Result<()> {
	/// let stats = ctx.get_stats().await?;
	/// // inspect fields on `stats`, e.g. `stats.total_commitments`
	/// # Ok(()) }
	/// ```
	pub async fn get_stats(&self) -> Result<operations::CommitmentStats> {
		operations::get_commitment_stats(&self.pool).await
	}

	// Delegation operations

	/// Persists a delegation record.
	///
	/// # Returns
	///
	/// The UUID of the saved delegation.
	///
	/// # Examples
	///
	/// ```no_run
	/// // Assume `ctx` is a `DatabaseContext` and `delegation` is a `SignedDelegation`.
	/// let id = tokio::runtime::Runtime::new().unwrap().block_on(async {
	///     ctx.save_delegation(&delegation).await.unwrap()
	/// });
	/// println!("saved id: {}", id);
	/// ```
	pub async fn save_delegation(&self, signed_delegation: &crate::types::SignedDelegation) -> Result<uuid::Uuid> {
		delegation_ops::save_delegation(&self.pool, signed_delegation).await
	}

	/// Retrieve all signed delegations for the specified slot.
	///
	/// # Returns
	///
	/// A `Vec<SignedDelegation>` containing all delegations associated with `slot`.
	///
	/// # Examples
	///
	/// ```no_run
	/// // `pool` is a previously created `PgPool`.
	/// let ctx = DatabaseContext::new(pool);
	/// let delegations = tokio::runtime::Runtime::new().unwrap().block_on(async {
	///     ctx.get_delegations_for_slot(42).await.unwrap()
	/// });
	/// ```
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
	/// ```no_run
	/// # use crate::db::DatabaseContext;
	/// # async fn example(db: &DatabaseContext) -> anyhow::Result<()> {
	/// let exists = db.delegation_exists_for_slot_and_committer(42, "0xdeadbeef").await?;
	/// println!("exists = {}", exists);
	/// # Ok(())
	/// # }
	/// ```
	pub async fn delegation_exists_for_slot_and_committer(&self, slot: u64, committer_address: &str) -> Result<bool> {
		delegation_ops::delegation_exists_for_slot_and_committer(&self.pool, slot, committer_address).await
	}

	/// Retrieves the signed delegation for a proposer at a specific slot.
	///
	/// # Examples
	///
	/// ```
	/// # use std::sync::Arc;
	/// # async fn example(db: Arc<crate::db::DatabaseContext>, proposer: crate::types::BlsPublicKey, slot: u64) {
	/// let result = db.get_delegation_by_proposer_slot(&proposer, slot).await.unwrap();
	/// // `result` is `Some(SignedDelegation)` if a delegation exists for the proposer at `slot`, otherwise `None`.
	/// # }
	/// ```
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
	/// ```no_run
	/// # use crate::db::DatabaseContext;
	/// # use crate::types::BlsPublicKey;
	/// # async fn example(ctx: &DatabaseContext, key: &BlsPublicKey) -> anyhow::Result<()> {
	/// let delegations = ctx.get_delegations_by_delegate(key).await?;
	/// assert!(delegations.iter().all(|d| &d.delegate_pubkey == key));
	/// # Ok(())
	/// # }
	/// ```
	pub async fn get_delegations_by_delegate(
		&self,
		delegate_pubkey: &crate::types::BlsPublicKey,
	) -> Result<Vec<crate::types::SignedDelegation>> {
		delegation_ops::get_delegations_by_delegate(&self.pool, delegate_pubkey).await
	}

	/// Saves multiple signed delegations in a single batch and returns their database IDs in input order.
	///
	/// The returned `Vec<uuid::Uuid>` contains the UUID for each saved delegation in the same order as the `delegations` slice.
	///
	/// # Examples
	///
	/// ```
	/// # async fn example(db: &crate::db::DatabaseContext) -> Result<(), Box<dyn std::error::Error>> {
	/// let delegations: Vec<crate::types::SignedDelegation> = vec![]; // build delegations
	/// let ids = db.save_delegations_batch(&delegations).await?;
	/// assert_eq!(ids.len(), delegations.len());
	/// # Ok(())
	/// # }
	/// ```
	pub async fn save_delegations_batch(
		&self,
		delegations: &[crate::types::SignedDelegation],
	) -> Result<Vec<uuid::Uuid>> {
		delegation_ops::save_delegations_batch(&self.pool, delegations).await
	}

	/// Retrieves aggregated delegation statistics.
	///
	/// Returns a `delegation_ops::DelegationStats` containing counts and aggregates for stored delegations.
	///
	/// # Examples
	///
	/// ```
	/// # #[tokio::test]
	/// # async fn doc_example_get_delegation_stats() {
	/// use crate::db::DatabaseContext;
	///
	/// // Use the testing constructor to get a context wired to a test database.
	/// let ctx = DatabaseContext::new_for_testing();
	/// let stats = ctx.get_delegation_stats().await.unwrap();
	/// let _ = stats; // use `stats` as needed
	/// # }
	/// ```
	pub async fn get_delegation_stats(&self) -> Result<delegation_ops::DelegationStats> {
		delegation_ops::get_delegation_stats(&self.pool).await
	}

	/// Deactivates delegations that have expired relative to the provided slot.
	///
	/// Returns the number of delegations that were deactivated.
	///
	/// # Examples
	///
	/// ```no_run
	/// # async fn example(db_ctx: &crate::db::DatabaseContext) -> anyhow::Result<()> {
	/// let deactivated = db_ctx.deactivate_expired_delegations(1_234_567).await?;
	/// println!("Deactivated {} delegations", deactivated);
	/// # Ok(())
	/// # }
	/// ```
	pub async fn deactivate_expired_delegations(&self, current_slot: u64) -> Result<u64> {
		delegation_ops::deactivate_expired_delegations(&self.pool, current_slot).await
	}

	// Slot congestion operations

	/// Retrieves the `SlotCongestion` record for `slot`, creating a new record initialized with
	/// `base_gas_price`, `total_gas_limit`, and `genesis_time` if none exists.
	///
	/// # Examples
	///
	/// ```no_run
	/// # async fn example(ctx: &crate::db::DatabaseContext) -> Result<(), Box<dyn std::error::Error>> {
	/// let congestion = ctx.get_or_create_slot_congestion(42, 100, 1_000_000, 0).await?;
	/// assert_eq!(congestion.slot, 42);
	/// # Ok(())
	/// # }
	/// ```
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
	/// ```no_run
	/// # async fn example(ctx: &crate::db::DatabaseContext) -> anyhow::Result<()> {
	/// let updated = ctx.update_slot_congestion_gas_usage(123, 10_000, 1.25).await?;
	/// // inspect returned record
	/// let _ = updated;
	/// # Ok(())
	/// # }
	/// ```
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
	/// ```
	/// # async fn run_example() -> anyhow::Result<()> {
	/// let ctx = crate::db::DatabaseContext::new_for_testing();
	/// let price = ctx.get_current_gas_price_for_slot(42).await?;
	/// match price {
	///     Some(p) => println!("Gas price for slot 42: {}", p),
	///     None => println!("No gas price recorded for slot 42"),
	/// }
	/// # Ok(())
	/// # }
	/// ```
	pub async fn get_current_gas_price_for_slot(&self, slot: u64) -> Result<Option<u64>> {
		slot_congestion_ops::get_current_gas_price_for_slot(&self.pool, slot).await
	}

	/// Retrieve aggregated slot congestion statistics.
	///
	/// # Examples
	///
	/// ```no_run
	/// # use crate::db::DatabaseContext;
	/// # async fn _example(db: &DatabaseContext) -> Result<(), anyhow::Error> {
	/// let stats = db.get_congestion_stats().await?;
	/// let _ = stats; // inspect congestion statistics
	/// # Ok(())
	/// # }
	/// ```
	pub async fn get_congestion_stats(&self) -> Result<slot_congestion_ops::CongestionStats> {
		slot_congestion_ops::get_congestion_stats(&self.pool).await
	}

	/// Removes slot congestion records older than the given number of hours.
	///
	/// Returns the number of removed slot congestion records.
	///
	/// # Examples
	///
	/// ```no_run
	/// # use crate::db::DatabaseContext;
	/// # async fn example(ctx: &DatabaseContext) -> Result<(), Box<dyn std::error::Error>> {
	/// let removed = ctx.cleanup_old_slot_congestion(24).await?;
	/// println!("Removed {} old records", removed);
	/// # Ok(()) }
	/// ```
	pub async fn cleanup_old_slot_congestion(&self, hours_to_keep: u32) -> Result<u64> {
		slot_congestion_ops::cleanup_old_slot_congestion(&self.pool, hours_to_keep).await
	}
}
