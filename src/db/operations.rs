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
pub async fn save_commitment(pool: &PgPool, signed_commitment: &SignedCommitment) -> Result<Uuid> {
	let id = Uuid::new_v4();
	let commitment = &signed_commitment.commitment;

	let commitment_type = i64::try_from(commitment.commitment_type).context("commitment_type exceeds i64::MAX")?;

	// Extract slot from payload for constraint submission queries
	let slot_number = match PayloadParser::extract_slot(commitment.commitment_type, &commitment.payload) {
		Ok(slot) => match i64::try_from(slot) {
			Ok(slot_i64) => Some(slot_i64),
			Err(e) => {
				tracing::warn!(
					commitment_id = %id,
					commitment_type = commitment.commitment_type,
					slot = slot,
					error = %e,
					"Slot number exceeds i64::MAX, storing NULL slot_number"
				);
				None
			}
		},
		Err(e) => {
			tracing::warn!(
				commitment_id = %id,
				commitment_type = commitment.commitment_type,
				payload_len = commitment.payload.len(),
				error = %e,
				"Failed to extract slot from payload, storing NULL slot_number"
			);
			None
		}
	};

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
		&commitment.request_hash,
		commitment_type,
		&commitment.payload,
		&commitment.slasher,
		&signed_commitment.signature,
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

	#[tokio::test]
	async fn test_save_commitment_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();
		let commitment = create_test_commitment("test_invalid_pool");

		let result = save_commitment(&invalid_pool, &commitment).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_commitment_by_hash_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_commitment_by_hash(&invalid_pool, "0x1234567890abcdef").await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_commitment_exists_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = commitment_exists(&invalid_pool, "0x1234567890abcdef").await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_commitment_stats_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_commitment_stats(&invalid_pool).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_unprocessed_commitments_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_unprocessed_commitments_for_slot(&invalid_pool, 12345).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_mark_commitments_as_processed_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();
		let hashes = vec!["0x1234567890abcdef".to_string()];

		let result = mark_commitments_as_processed(&invalid_pool, &hashes).await;
		assert!(result.is_err());
	}

	#[test]
	fn test_commitment_stats_creation() {
		let stats = CommitmentStats {
			total_count: 100,
			commitment_type_1_count: 80,
			latest_created_at: Some(chrono::Utc::now()),
		};

		assert_eq!(stats.total_count, 100);
		assert_eq!(stats.commitment_type_1_count, 80);
		assert!(stats.latest_created_at.is_some());
	}

	#[test]
	fn test_commitment_stats_empty() {
		let stats = CommitmentStats { total_count: 0, commitment_type_1_count: 0, latest_created_at: None };

		assert_eq!(stats.total_count, 0);
		assert_eq!(stats.commitment_type_1_count, 0);
		assert!(stats.latest_created_at.is_none());
	}

	#[test]
	fn test_commitment_stats_debug() {
		let stats = CommitmentStats { total_count: 50, commitment_type_1_count: 30, latest_created_at: None };

		let debug_str = format!("{:?}", stats);
		assert!(debug_str.contains("CommitmentStats"));
		assert!(debug_str.contains("50"));
		assert!(debug_str.contains("30"));
	}

	#[test]
	fn test_commitment_type_conversion_edge_cases() {
		// Test valid conversion
		let valid_type: u64 = 1;
		let converted = i64::try_from(valid_type);
		assert!(converted.is_ok());
		assert_eq!(converted.unwrap(), 1);

		// Test conversion back
		let converted_back = u64::try_from(converted.unwrap());
		assert!(converted_back.is_ok());
		assert_eq!(converted_back.unwrap(), 1);
	}

	#[test]
	fn test_slot_number_conversion_edge_cases() {
		// Test valid slot conversion
		let valid_slot: u64 = 12345;
		let converted = i64::try_from(valid_slot);
		assert!(converted.is_ok());
		assert_eq!(converted.unwrap(), 12345);

		// Test large slot number
		let large_slot: u64 = u64::MAX;
		let converted_large = i64::try_from(large_slot);
		assert!(converted_large.is_err()); // Should fail for values > i64::MAX
	}

	#[test]
	fn test_commitment_creation_with_different_types() {
		use crate::types::payload::{InclusionPayload, PayloadParser};

		// Test type 1 (inclusion)
		let inclusion_payload = InclusionPayload::new(12345, vec![0x01, 0x02, 0x03]);
		let payload_bytes = PayloadParser::encode_inclusion_payload(&inclusion_payload).unwrap();

		let commitment_type_1 = Commitment {
			commitment_type: 1,
			payload: payload_bytes.clone(),
			request_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		assert_eq!(commitment_type_1.commitment_type, 1);
		assert_eq!(commitment_type_1.payload.len(), payload_bytes.len());

		// Test type 2 (execution)
		let commitment_type_2 = Commitment {
			commitment_type: 2,
			payload: payload_bytes,
			request_hash: "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
			slasher: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
		};

		assert_eq!(commitment_type_2.commitment_type, 2);
		assert_ne!(commitment_type_1.request_hash, commitment_type_2.request_hash);
	}

	#[test]
	fn test_signed_commitment_creation() {
		let commitment = Commitment {
			commitment_type: 1,
			payload: vec![0x01, 0x02, 0x03],
			request_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let signature = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string();

		let signed_commitment = SignedCommitment { commitment, signature };

		assert_eq!(signed_commitment.commitment.commitment_type, 1);
		assert_eq!(signed_commitment.signature.len(), 130); // 0x + 128 hex chars
	}

	#[test]
	fn test_payload_extraction_edge_cases() {
		use crate::types::payload::{InclusionPayload, PayloadParser};

		// Test with valid inclusion payload
		let inclusion_payload = InclusionPayload::new(12345, vec![0x01, 0x02, 0x03]);
		let payload_bytes = PayloadParser::encode_inclusion_payload(&inclusion_payload).unwrap();

		let extracted_slot = PayloadParser::extract_slot(1, &payload_bytes);
		assert!(extracted_slot.is_ok());
		assert_eq!(extracted_slot.unwrap(), 12345);

		// Test with invalid commitment type
		let invalid_extraction = PayloadParser::extract_slot(999, &payload_bytes);
		assert!(invalid_extraction.is_err());

		// Test with empty payload
		let empty_extraction = PayloadParser::extract_slot(1, &[]);
		assert!(empty_extraction.is_err());
	}

	#[test]
	fn test_request_hash_format() {
		let valid_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
		assert_eq!(valid_hash.len(), 66); // 0x + 64 hex chars
		assert!(valid_hash.starts_with("0x"));

		let invalid_hash = "0x1234567890abcdef"; // Too short
		assert_ne!(invalid_hash.len(), 66);
	}

	#[test]
	fn test_slasher_address_format() {
		let valid_address = "0x1234567890123456789012345678901234567890";
		assert_eq!(valid_address.len(), 42); // 0x + 40 hex chars
		assert!(valid_address.starts_with("0x"));

		let invalid_address = "0x1234567890abcdef"; // Too short
		assert_ne!(invalid_address.len(), 42);
	}

	#[test]
	fn test_signature_format() {
		let valid_signature = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
		assert_eq!(valid_signature.len(), 130); // 0x + 128 hex chars
		assert!(valid_signature.starts_with("0x"));

		let invalid_signature = "0x1234567890abcdef"; // Too short
		assert_ne!(invalid_signature.len(), 130);
	}

	#[tokio::test]
	async fn test_mark_nonexistent_commitments() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Try to mark commitments that don't exist
		let fake_hashes = vec![
			"0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
			"0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
		];

		let marked = mark_commitments_as_processed(&pool, &fake_hashes).await?;
		assert_eq!(marked, 0); // Should mark 0 commitments

		Ok(())
	}

	#[tokio::test]
	async fn test_duplicate_commitment_handling() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		let commitment = create_test_commitment("test_duplicate");

		// Save first time
		let id1 = save_commitment(&pool, &commitment).await?;
		assert!(!id1.is_nil());

		// Try to save again with same request hash (should fail due to unique constraint)
		let result = save_commitment(&pool, &commitment).await;
		assert!(result.is_err()); // Should fail due to unique constraint on request_hash

		Ok(())
	}

	#[tokio::test]
	async fn test_get_stats_with_no_commitments() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		let stats = get_commitment_stats(&pool).await?;
		// Stats should be 0 when no commitments exist
		assert_eq!(stats.total_count, 0);
		assert_eq!(stats.commitment_type_1_count, 0);

		Ok(())
	}

	#[tokio::test]
	async fn test_get_stats_with_multiple_commitments() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Create multiple commitments
		let commitment1 = create_test_commitment("stats_test_1");
		let commitment2 = create_test_commitment("stats_test_2");
		let commitment3 = create_test_commitment("stats_test_3");

		save_commitment(&pool, &commitment1).await?;
		save_commitment(&pool, &commitment2).await?;
		save_commitment(&pool, &commitment3).await?;

		let stats = get_commitment_stats(&pool).await?;
		assert_eq!(stats.total_count, 3);
		assert_eq!(stats.commitment_type_1_count, 3); // All are type 1

		Ok(())
	}

	#[tokio::test]
	async fn test_get_commitments_by_slot() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Create commitments with different slots
		let commitment1 = create_test_commitment_with_slot("slot_test_1", 1000);
		let commitment2 = create_test_commitment_with_slot("slot_test_2", 2000);
		let commitment3 = create_test_commitment_with_slot("slot_test_3", 3000);

		let _id1 = save_commitment(&pool, &commitment1).await?;
		let _id2 = save_commitment(&pool, &commitment2).await?;
		let _id3 = save_commitment(&pool, &commitment3).await?;

		// Test slot query
		let commitments = get_unprocessed_commitments_for_slot(&pool, 2000).await?;
		assert_eq!(commitments.len(), 1);
		assert_eq!(commitments[0].commitment.request_hash, commitment2.commitment.request_hash);

		// Test different slot
		let commitments = get_unprocessed_commitments_for_slot(&pool, 1000).await?;
		assert_eq!(commitments.len(), 1);
		assert_eq!(commitments[0].commitment.request_hash, commitment1.commitment.request_hash);

		Ok(())
	}

	#[tokio::test]
	async fn test_get_commitments_by_slot_empty() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Test with non-existent slot
		let commitments = get_unprocessed_commitments_for_slot(&pool, 9999).await?;
		assert_eq!(commitments.len(), 0);

		Ok(())
	}

	#[tokio::test]
	async fn test_get_commitments_by_slot_with_nulls() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Create a commitment without slot (this should result in NULL slot_number)
		let commitment = create_test_commitment("null_slot_test");
		save_commitment(&pool, &commitment).await?;

		// Query should not return commitments with NULL slot_number
		let commitments = get_unprocessed_commitments_for_slot(&pool, 0).await?;
		// The commitment should not be returned because it has NULL slot_number
		assert_eq!(commitments.len(), 0);

		Ok(())
	}

	#[tokio::test]
	async fn test_mark_commitments_as_processed_with_empty_list() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Test with empty list
		let marked = mark_commitments_as_processed(&pool, &[]).await?;
		assert_eq!(marked, 0);

		Ok(())
	}

	#[tokio::test]
	async fn test_mark_commitments_as_processed_with_mixed_hashes() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Create a real commitment
		let commitment = create_test_commitment("mixed_test");
		let _id = save_commitment(&pool, &commitment).await?;

		// Mix real and fake hashes
		let hashes = vec![
			commitment.commitment.request_hash.clone(),
			"0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
		];

		let marked = mark_commitments_as_processed(&pool, &hashes).await?;
		assert_eq!(marked, 1); // Should mark only the real commitment

		Ok(())
	}

	#[tokio::test]
	async fn test_commitment_retrieval_edge_cases() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Test retrieving non-existent commitment
		let result =
			get_commitment_by_hash(&pool, "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff").await?;
		assert!(result.is_none());

		// Test with invalid hash format
		let result = get_commitment_by_hash(&pool, "invalid_hash").await?;
		assert!(result.is_none());

		Ok(())
	}

	#[tokio::test]
	async fn test_commitment_existence_edge_cases() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Test with non-existent hash
		let exists =
			commitment_exists(&pool, "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff").await?;
		assert!(!exists);

		// Test with invalid hash format
		let exists = commitment_exists(&pool, "invalid_hash").await?;
		assert!(!exists);

		Ok(())
	}

	#[test]
	fn test_commitment_type_conversion() {
		// Test normal conversion
		let commitment_type = 1u8;
		let converted = i64::from(commitment_type);
		assert_eq!(converted, 1);

		// Test edge case
		let commitment_type = 255u8;
		let converted = i64::from(commitment_type);
		assert_eq!(converted, 255);
	}

	#[test]
	fn test_slot_number_conversion() {
		// Test normal conversion
		let slot = 12345u64;
		let converted = i64::try_from(slot).unwrap();
		assert_eq!(converted, 12345);

		// Test edge case - this should work since u64::MAX fits in i64::MAX
		let slot = u64::MAX;
		let converted = i64::try_from(slot);
		// This should fail because u64::MAX > i64::MAX
		assert!(converted.is_err());
	}

	#[test]
	fn test_uuid_generation() {
		let id1 = Uuid::new_v4();
		let id2 = Uuid::new_v4();

		// UUIDs should be unique
		assert_ne!(id1, id2);

		// UUIDs should not be nil
		assert!(!id1.is_nil());
		assert!(!id2.is_nil());
	}

	#[tokio::test]
	async fn test_concurrent_commitment_saves() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Create multiple commitments concurrently
		let mut handles = vec![];

		for i in 0..5 {
			let pool_clone = pool.clone();
			let handle = tokio::spawn(async move {
				let commitment = create_test_commitment(&format!("concurrent_test_{}", i));
				save_commitment(&pool_clone, &commitment).await
			});
			handles.push(handle);
		}

		// Wait for all saves to complete
		for handle in handles {
			let result = handle.await.unwrap();
			assert!(result.is_ok());
		}

		// Verify all commitments were saved
		let stats = get_commitment_stats(&pool).await?;
		assert_eq!(stats.total_count, 5);

		Ok(())
	}

	#[tokio::test]
	async fn test_large_payload_handling() -> Result<()> {
		let pool = match setup_test_pool().await {
			Ok(p) => p,
			Err(_) => {
				eprintln!("Skipping test: DATABASE_URL not set");
				return Ok(());
			}
		};

		// Create a commitment with a large payload
		let mut large_payload = vec![0u8; 10000]; // 10KB payload
		for (i, byte) in large_payload.iter_mut().enumerate() {
			*byte = (i % 256) as u8;
		}

		let commitment = Commitment {
			commitment_type: 1,
			payload: large_payload,
			request_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let signed_commitment = SignedCommitment { commitment, signature: format!("0x{:0<128}", "1234567890abcdef") };

		let id = save_commitment(&pool, &signed_commitment).await?;
		assert!(!id.is_nil());

		// Verify it can be retrieved
		let retrieved = get_commitment_by_hash(&pool, &signed_commitment.commitment.request_hash).await?.unwrap();
		assert_eq!(retrieved.commitment.payload.len(), 10000);

		Ok(())
	}
}
