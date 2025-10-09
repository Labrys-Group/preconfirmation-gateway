//! Database operations for commitment storage and retrieval
//!
//! This module provides SQLx-based database operations for managing commitments
//! according to the Gateway specification.

use anyhow::{Context, Result};
use sqlx::{PgPool, types::chrono};
use std::convert::TryFrom;
use uuid::Uuid;

use crate::types::{Commitment, PayloadParser, SignedCommitment};

/// Insert a SignedCommitment into the commitments table and record its payload slot for later constraint submission queries.
///
/// Extracts the slot number from the commitment payload when present and stores it in the row.
///
/// # Returns
///
/// `Uuid` of the newly inserted commitment row.
///
/// # Examples
///
/// ```ignoreno_run
/// # use sqlx::PgPool;
/// # use uuid::Uuid;
/// # use crate::types::SignedCommitment;
/// # async fn doc_example(pool: &PgPool, signed: &SignedCommitment) {
/// let id: Uuid = crate::db::operations::save_commitment(pool, signed).await.unwrap();
/// println!("inserted id = {}", id);
/// # }
/// ```ignore
pub async fn save_commitment(pool: &PgPool, signed_commitment: &SignedCommitment) -> Result<Uuid> {
	let id = Uuid::new_v4();
	let commitment = &signed_commitment.commitment;

	let commitment_type = i64::try_from(commitment.commitment_type).context("commitment_type exceeds i64::MAX")?;

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

/// Look up a signed commitment by its request hash.
///
/// Returns the complete `SignedCommitment` if a row with the given `request_hash` exists in the database,
/// otherwise returns `None`.
///
/// # Examples
///
/// ```ignore
/// # async fn _example(pool: &sqlx::PgPool) -> anyhow::Result<()> {
/// let maybe_signed = crate::db::operations::get_commitment_by_hash(pool, "request_hash_value").await?;
/// if let Some(signed) = maybe_signed {
///     // use `signed.commitment` and `signed.signature`
/// }
/// # Ok(())
/// # }
/// ```ignore
///
/// # Returns
///
/// `Some(SignedCommitment)` if a matching commitment exists, `None` otherwise.
pub async fn get_commitment_by_hash(pool: &PgPool, request_hash: &str) -> Result<Option<SignedCommitment>> {
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
			let commitment_type = u64::try_from(row.commitment_type).context("stored commitment_type is negative")?;

			let commitment = Commitment {
				commitment_type,
				payload: row.payload,
				request_hash: row.request_hash,
				slasher: row.slasher,
			};

			let signed_commitment = SignedCommitment { commitment, signature: row.signature };

			Ok(Some(signed_commitment))
		}
		None => Ok(None),
	}
}

/// Determines whether a commitment with the given request hash exists.
///
/// # Returns
///
/// `true` if a row with the provided `request_hash` exists in the `commitments` table, `false` otherwise.
///
/// # Examples
///
/// ```ignoreno_run
/// # use sqlx::PgPool;
/// # async fn example(pool: PgPool) -> Result<(), Box<dyn std::error::Error>> {
/// let exists = crate::db::operations::commitment_exists(&pool, "some-request-hash").await?;
/// println!("exists = {}", exists);
/// # Ok(())
/// # }
/// ```ignore
pub async fn commitment_exists(pool: &PgPool, request_hash: &str) -> Result<bool> {
	let row = sqlx::query!("SELECT EXISTS(SELECT 1 FROM commitments WHERE request_hash = $1)", request_hash)
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

/// Retrieve aggregated statistics about stored commitments.
///
/// Returns a `CommitmentStats` struct containing:
/// - `total_count`: total number of commitments in the table,
/// - `commitment_type_1_count`: number of commitments whose `commitment_type` equals 1,
/// - `latest_created_at`: timestamp of the most recently created commitment, or `None` if there are no rows.
///
/// # Examples
///
/// ```ignoreno_run
/// # async fn example(pool: sqlx::PgPool) -> anyhow::Result<()> {
/// let stats = crate::db::operations::get_commitment_stats(&pool).await?;
/// println!("total: {}", stats.total_count);
/// # Ok(())
/// # }
/// ```ignore
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

/// Fetches unprocessed commitments for the given slot in ascending creation order.
///
/// Returns a vector of `SignedCommitment` for rows whose `slot_number` matches `slot` and whose
/// `constraint_processed` flag is `false`.
///
/// # Examples
///
/// ```ignore
/// # async fn example(pool: &sqlx::PgPool) -> anyhow::Result<()> {
/// let slot = 42u64;
/// let commitments = crate::db::operations::get_unprocessed_commitments_for_slot(pool, slot).await?;
/// assert!(commitments.iter().all(|c| c.commitment.request_hash.len() > 0));
/// # Ok(())
/// # }
/// ```ignore
pub async fn get_unprocessed_commitments_for_slot(pool: &PgPool, slot: u64) -> Result<Vec<SignedCommitment>> {
	let slot_i64 = i64::try_from(slot).context("slot exceeds i64::MAX")?;

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
		let commitment_type = u64::try_from(row.commitment_type).context("stored commitment_type is negative")?;

		let commitment =
			Commitment { commitment_type, payload: row.payload, request_hash: row.request_hash, slasher: row.slasher };

		let signed_commitment = SignedCommitment { commitment, signature: row.signature };

		commitments.push(signed_commitment);
	}

	Ok(commitments)
}

/// Mark commitments identified by the given request hashes as processed.
///
/// Sets `constraint_processed = TRUE` for all rows whose `request_hash` matches any value in
/// `request_hashes` and returns the number of rows that were updated.
///
/// # Examples
///
/// ```ignoreno_run
/// # use sqlx::PgPool;
/// # use uuid::Uuid;
/// # async fn example(pool: &PgPool) -> Result<(), anyhow::Error> {
/// let hashes = vec!["req_hash_1".to_string(), "req_hash_2".to_string()];
/// let updated = mark_commitments_as_processed(pool, &hashes).await?;
/// println!("Marked {} commitments as processed", updated);
/// # Ok(())
/// # }
/// ```ignore
pub async fn mark_commitments_as_processed(pool: &PgPool, request_hashes: &[String]) -> Result<u64> {
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

	/// Creates a PostgreSQL connection pool configured for use by tests.
	///
	/// This function is a placeholder intended to establish and return a `PgPool` connected to the
	/// test database; replace its body with test-specific connection logic.
	///
	/// # Examples
	///
	/// ```ignore
	/// # async fn run() {
	/// let pool = setup_test_pool().await;
	/// // use `pool` for test queries
	/// # }
	/// ```ignore
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
		let retrieved = get_commitment_by_hash(&pool, &commitment.request_hash).await.unwrap().unwrap();

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
