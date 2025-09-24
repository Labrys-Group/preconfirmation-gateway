//! Database module for SQLx-based PostgreSQL operations
//!
//! This module provides database connectivity and operations for the preconfirmation gateway.
//! It uses SQLx for compile-time checked SQL queries and async database operations.

pub mod operations;

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

	info!("Connecting to database: {}", database_url);

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
}
