//! Database operations for commitment storage and retrieval
//!
//! This module provides SQLx-based database operations for managing commitments
//! according to the Gateway specification.

use anyhow::{Context, Result};
use sqlx::{types::chrono, PgPool};
use std::convert::TryFrom;
use uuid::Uuid;

use crate::types::{Commitment, SignedCommitment, PayloadParser};

/// Save a signed commitment to the database
///
/// This function stores a complete SignedCommitment with all its fields
/// in the commitments table for later retrieval.
/// It also extracts and stores the slot number from the payload for constraint submission queries.
pub async fn save_commitment(
	pool: &PgPool,
	signed_commitment: &SignedCommitment,
) -> Result<Uuid> {
	let id = Uuid::new_v4();
	let commitment = &signed_commitment.commitment;

	let commitment_type = i64::try_from(commitment.commitment_type)
		.context("commitment_type exceeds i64::MAX")?;

	// Extract slot from payload for constraint submission queries
	let slot_number = PayloadParser::extract_slot(commitment.commitment_type, &commitment.payload)
		.ok()
		.and_then(|slot| i64::try_from(slot).ok());

	let row = sqlx::query!(
		r#"
		INSERT INTO commitments (
			id,
			request_hash,
			commitment_type,
			payload,
			slasher,
			signature,
			slot_number
		)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		RETURNING id
		"#,
		id,
		commitment.request_hash,
		commitment_type,
		commitment.payload,
		commitment.slasher,
		signed_commitment.signature,
		slot_number
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
			let commitment_type = u64::try_from(row.commitment_type)
				.context("stored commitment_type is negative")?;

			let commitment = Commitment {
				commitment_type,
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

/// Get unprocessed commitments for a specific slot
///
/// This is used by the constraint submission service to find commitments
/// that need to be converted to constraints and submitted to the relay.
pub async fn get_unprocessed_commitments_for_slot(
	pool: &PgPool,
	slot: u64,
) -> Result<Vec<SignedCommitment>> {
	let slot_i64 = i64::try_from(slot)
		.context("slot exceeds i64::MAX")?;

	let rows = sqlx::query!(
		r#"
		SELECT
			request_hash,
			commitment_type,
			payload,
			slasher,
			signature,
			created_at
		FROM commitments
		WHERE slot_number = $1
		  AND constraint_processed = FALSE
		ORDER BY created_at ASC
		"#,
		slot_i64
	)
	.fetch_all(pool)
	.await
	.context("Failed to query unprocessed commitments for slot")?;

	let mut commitments = Vec::new();
	for row in rows {
		let commitment_type = u64::try_from(row.commitment_type)
			.context("stored commitment_type is negative")?;

		let commitment = Commitment {
			commitment_type,
			payload: row.payload,
			request_hash: row.request_hash,
			slasher: row.slasher,
		};

		let signed_commitment = SignedCommitment {
			commitment,
			signature: row.signature,
		};

		commitments.push(signed_commitment);
	}

	Ok(commitments)
}

/// Mark commitments as processed after they've been converted to constraints
///
/// This prevents duplicate constraint submissions for the same commitment.
pub async fn mark_commitments_as_processed(
	pool: &PgPool,
	request_hashes: &[String],
) -> Result<u64> {
	if request_hashes.is_empty() {
		return Ok(0);
	}

	let result = sqlx::query!(
		r#"
		UPDATE commitments
		SET constraint_processed = TRUE
		WHERE request_hash = ANY($1)
		"#,
		request_hashes
	)
	.execute(pool)
	.await
	.context("Failed to mark commitments as processed")?;

	Ok(result.rows_affected())
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