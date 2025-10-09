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
	use sqlx::{PgPool, postgres::PgPoolOptions};

	/// Creates a PostgreSQL connection pool configured for use by tests.
	///
	/// Connects to the database specified by DATABASE_URL environment variable.
	/// Tests will be skipped if DATABASE_URL is not set.
	async fn setup_test_pool() -> Result<PgPool> {
		let database_url = std::env::var("DATABASE_URL")
			.context("DATABASE_URL environment variable not set. Set it to run database tests.")?;

		let pool = PgPoolOptions::new()
			.max_connections(5)
			.connect(&database_url)
			.await
			.context("Failed to connect to test database")?;

		Ok(pool)
	}

	/// Helper to create a test commitment with a unique request hash and slot
	fn create_test_commitment_with_slot(suffix: &str, slot: u64) -> SignedCommitment {
		use crate::types::payload::{InclusionPayload, PayloadParser};
		use std::time::{SystemTime, UNIX_EPOCH};

		// Create a unique 32-byte hash using suffix + timestamp for uniqueness
		let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let unique_str = format!("{}{}", suffix, timestamp);
		let suffix_hex = hex::encode(unique_str.as_bytes());
		// Ensure exactly 64 hex chars by truncating or padding
		let hash_body = if suffix_hex.len() >= 64 { &suffix_hex[..64] } else { &format!("{:0<64}", suffix_hex) };
		let hash_hex = format!("0x{}", hash_body);

		// Create a valid inclusion payload with the given slot
		let inclusion_payload = InclusionPayload::new(slot, vec![0x01, 0x02, 0x03]);
		let payload_bytes = PayloadParser::encode_inclusion_payload(&inclusion_payload).unwrap();

		let commitment = Commitment {
			commitment_type: 1,
			payload: payload_bytes,
			request_hash: hash_hex,
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		SignedCommitment {
			commitment,
			// ECDSA signature is 0x + 128 hex chars (64 bytes) per migration 002
			signature: format!("0x{:0<128}", "1234567890abcdef"),
		}
	}

	/// Helper to create a test commitment with a unique request hash (random slot)
	fn create_test_commitment(suffix: &str) -> SignedCommitment {
		create_test_commitment_with_slot(suffix, 12345)
	}

	#[tokio::test]
	async fn test_save_and_retrieve_commitment() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		let signed_commitment = create_test_commitment("test_save_retrieve");

		// Save commitment
		let id = save_commitment(&pool, &signed_commitment).await?;
		assert!(!id.is_nil());

		// Retrieve commitment
		let retrieved = get_commitment_by_hash(&pool, &signed_commitment.commitment.request_hash).await?.unwrap();

		assert_eq!(retrieved.commitment.commitment_type, signed_commitment.commitment.commitment_type);
		assert_eq!(retrieved.commitment.payload, signed_commitment.commitment.payload);
		assert_eq!(retrieved.commitment.request_hash, signed_commitment.commitment.request_hash);
		assert_eq!(retrieved.commitment.slasher, signed_commitment.commitment.slasher);
		assert_eq!(retrieved.signature, signed_commitment.signature);

		Ok(())
	}

	#[tokio::test]
	async fn test_commitment_exists() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		let signed_commitment = create_test_commitment("test_exists");

		// Should not exist initially
		assert!(!commitment_exists(&pool, &signed_commitment.commitment.request_hash).await?);

		// Save commitment
		save_commitment(&pool, &signed_commitment).await?;

		// Should now exist
		assert!(commitment_exists(&pool, &signed_commitment.commitment.request_hash).await?);

		Ok(())
	}

	#[tokio::test]
	async fn test_get_commitment_stats() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Get initial stats
		let stats_before = get_commitment_stats(&pool).await?;

		// Add a type 1 commitment
		let signed_commitment = create_test_commitment("test_stats");
		save_commitment(&pool, &signed_commitment).await?;

		// Get updated stats
		let stats_after = get_commitment_stats(&pool).await?;

		// Verify counts increased (may be more than 1 if tests run in parallel)
		assert!(stats_after.total_count > stats_before.total_count, "Total count should increase by at least 1");
		assert!(
			stats_after.commitment_type_1_count > stats_before.commitment_type_1_count,
			"Type 1 count should increase by at least 1"
		);
		assert!(stats_after.latest_created_at.is_some());

		// Latest timestamp should be newer
		if let Some(before_time) = stats_before.latest_created_at {
			assert!(stats_after.latest_created_at.unwrap() >= before_time);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_get_unprocessed_commitments_for_slot() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		let test_slot: u64 = 999999; // Use a unique slot number for testing

		// Should be empty initially (or at least get baseline count)
		let commitments = get_unprocessed_commitments_for_slot(&pool, test_slot).await?;
		let initial_count = commitments.len();

		// Add a commitment for this slot with proper payload
		let signed_commitment = create_test_commitment_with_slot("test_unprocessed_slot", test_slot);
		save_commitment(&pool, &signed_commitment).await?;

		// Should now have one more unprocessed commitment
		let commitments = get_unprocessed_commitments_for_slot(&pool, test_slot).await?;
		assert_eq!(commitments.len(), initial_count + 1);

		Ok(())
	}

	#[tokio::test]
	async fn test_mark_commitments_as_processed() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		let test_slot: u64 = 888888;

		// Create and save a commitment with proper payload
		let signed_commitment = create_test_commitment_with_slot("test_mark_processed", test_slot);
		save_commitment(&pool, &signed_commitment).await?;

		// Should be in unprocessed list
		let unprocessed = get_unprocessed_commitments_for_slot(&pool, test_slot).await?;
		let before_count = unprocessed.len();
		assert!(before_count > 0, "Expected at least one unprocessed commitment for slot {}", test_slot);

		// Mark as processed
		let hashes = vec![signed_commitment.commitment.request_hash.clone()];
		let marked = mark_commitments_as_processed(&pool, &hashes).await?;
		assert!(marked > 0);

		// Should no longer be in unprocessed list
		let unprocessed = get_unprocessed_commitments_for_slot(&pool, test_slot).await?;
		assert_eq!(unprocessed.len(), before_count - 1);

		Ok(())
	}

	#[tokio::test]
	async fn test_mark_empty_array() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Should return 0 for empty array
		let marked = mark_commitments_as_processed(&pool, &[]).await?;
		assert_eq!(marked, 0);

		Ok(())
	}

	#[tokio::test]
	async fn test_get_nonexistent_commitment() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Query for a commitment that definitely doesn't exist
		let fake_hash = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
		let result = get_commitment_by_hash(&pool, fake_hash).await?;

		// Should return None
		assert!(result.is_none(), "Expected None for non-existent commitment");

		Ok(())
	}
}
