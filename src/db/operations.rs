//! Database operations for commitment storage and retrieval
//!
//! This module provides SQLx-based database operations for managing commitments
//! according to the Gateway specification.

use anyhow::{Context, Result};
use sqlx::{types::chrono, PgPool};
use uuid::Uuid;

use crate::types::{Commitment, SignedCommitment};

/// Save a signed commitment to the database
///
/// This function stores a complete SignedCommitment with all its fields
/// in the commitments table for later retrieval.
pub async fn save_commitment(
	pool: &PgPool,
	signed_commitment: &SignedCommitment,
) -> Result<Uuid> {
	let id = Uuid::new_v4();
	let commitment = &signed_commitment.commitment;

	let row = sqlx::query!(
		r#"
		INSERT INTO commitments (
			id,
			request_hash,
			commitment_type,
			payload,
			slasher,
			signature
		)
		VALUES ($1, $2, $3, $4, $5, $6)
		RETURNING id
		"#,
		id,
		commitment.request_hash,
		commitment.commitment_type as i64,
		commitment.payload,
		commitment.slasher,
		signed_commitment.signature
	)
	.fetch_one(pool)
	.await
	.context("Failed to insert commitment into database")?;

	Ok(row.id)
}

/// Retrieve a signed commitment by request hash
///
/// This function looks up a commitment using its request_hash and returns
/// the complete SignedCommitment if found.
pub async fn get_commitment_by_hash(
	pool: &PgPool,
	request_hash: &str,
) -> Result<Option<SignedCommitment>> {
	let row = sqlx::query!(
		r#"
		SELECT
			request_hash,
			commitment_type,
			payload,
			slasher,
			signature,
			created_at
		FROM commitments
		WHERE request_hash = $1
		"#,
		request_hash
	)
	.fetch_optional(pool)
	.await
	.context("Failed to query commitment from database")?;

	match row {
		Some(row) => {
			let commitment = Commitment {
				commitment_type: row.commitment_type as u64,
				payload: row.payload,
				request_hash: row.request_hash,
				slasher: row.slasher,
			};

			let signed_commitment = SignedCommitment {
				commitment,
				signature: row.signature,
			};

			Ok(Some(signed_commitment))
		}
		None => Ok(None),
	}
}

/// Check if a commitment with the given request hash already exists
///
/// This is useful for preventing duplicate commitments
pub async fn commitment_exists(pool: &PgPool, request_hash: &str) -> Result<bool> {
	let row = sqlx::query!(
		"SELECT EXISTS(SELECT 1 FROM commitments WHERE request_hash = $1)",
		request_hash
	)
	.fetch_one(pool)
	.await
	.context("Failed to check if commitment exists")?;

	Ok(row.exists.unwrap_or(false))
}

/// Get commitment statistics for monitoring/debugging
///
/// Returns basic metrics about stored commitments
#[derive(Debug)]
pub struct CommitmentStats {
	pub total_count: i64,
	pub commitment_type_1_count: i64,
	pub latest_created_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn get_commitment_stats(pool: &PgPool) -> Result<CommitmentStats> {
	let row = sqlx::query!(
		r#"
		SELECT
			COUNT(*) as total_count,
			COUNT(*) FILTER (WHERE commitment_type = 1) as type_1_count,
			MAX(created_at) as latest_created_at
		FROM commitments
		"#
	)
	.fetch_one(pool)
	.await
	.context("Failed to get commitment statistics")?;

	Ok(CommitmentStats {
		total_count: row.total_count.unwrap_or(0),
		commitment_type_1_count: row.type_1_count.unwrap_or(0),
		latest_created_at: row.latest_created_at,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::{Commitment, SignedCommitment};
	use sqlx::PgPool;

	async fn setup_test_pool() -> PgPool {
		// This would connect to a test database
		// For now, we'll skip actual DB tests until we have the database running
		todo!("Setup test database connection")
	}

	#[tokio::test]
	#[ignore] // Ignore until we have test database setup
	async fn test_save_and_retrieve_commitment() {
		let pool = setup_test_pool().await;

		let commitment = Commitment {
			commitment_type: 1,
			payload: vec![1, 2, 3, 4],
			request_hash: "0x1234567890123456789012345678901234567890123456789012345678901234".to_string(),
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let signed_commitment = SignedCommitment {
			commitment: commitment.clone(),
			signature: "0x1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890".to_string(),
		};

		// Save commitment
		let id = save_commitment(&pool, &signed_commitment).await.unwrap();
		assert!(!id.is_nil());

		// Retrieve commitment
		let retrieved = get_commitment_by_hash(&pool, &commitment.request_hash)
			.await
			.unwrap()
			.unwrap();

		assert_eq!(retrieved.commitment.commitment_type, commitment.commitment_type);
		assert_eq!(retrieved.commitment.payload, commitment.payload);
		assert_eq!(retrieved.commitment.request_hash, commitment.request_hash);
		assert_eq!(retrieved.commitment.slasher, commitment.slasher);
		assert_eq!(retrieved.signature, signed_commitment.signature);
	}

	#[tokio::test]
	#[ignore] // Ignore until we have test database setup
	async fn test_commitment_exists() {
		let pool = setup_test_pool().await;

		let request_hash = "0x1234567890123456789012345678901234567890123456789012345678901234";

		// Should not exist initially
		assert!(!commitment_exists(&pool, request_hash).await.unwrap());

		// Create and save a commitment
		let commitment = Commitment {
			commitment_type: 1,
			payload: vec![1, 2, 3, 4],
			request_hash: request_hash.to_string(),
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let signed_commitment = SignedCommitment {
			commitment,
			signature: "0x1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890".to_string(),
		};

		save_commitment(&pool, &signed_commitment).await.unwrap();

		// Should now exist
		assert!(commitment_exists(&pool, request_hash).await.unwrap());
	}
}