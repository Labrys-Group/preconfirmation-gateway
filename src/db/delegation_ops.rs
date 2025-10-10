//! Database operations for delegation management
//!
//! This module provides SQLx-based database operations for managing SignedDelegation
//! messages according to the Gateway specification.

use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::types::delegation::{BlsPublicKey, BlsSignature, DelegationMessage, SignedDelegation};

/// Persist a SignedDelegation record and return the inserted row identifier.
///
/// Inserts the delegation fields into the delegations table and marks the row as active.
///
/// # Examples
///
/// ```ignore
/// # async fn example(pool: &sqlx::PgPool, delegation: super::SignedDelegation) -> Result<(), Box<dyn std::error::Error>> {
/// let id = crate::db::delegation_ops::save_delegation(pool, &delegation).await?;
/// // `id` is the database identifier for the inserted delegation
/// assert!(!id.to_string().is_empty());
/// # Ok(())
/// # }
/// ```ignore
pub async fn save_delegation(pool: &PgPool, signed_delegation: &SignedDelegation) -> Result<Uuid> {
	let id = Uuid::new_v4();
	let message = &signed_delegation.message;

	let row = sqlx::query!(
		r#"
		INSERT INTO delegations (
			id,
			proposer_pubkey,
			delegate_pubkey,
			committer_address,
			slot_number,
			signature,
			is_active
		)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		RETURNING id
		"#,
		id,
		&message.proposer.0[..], // Convert BlsPublicKey to &[u8]
		&message.delegate.0[..], // Convert BlsPublicKey to &[u8]
		message.committer,
		message.slot as i64,
		&signed_delegation.signature.0[..], // Convert BlsSignature to &[u8]
		true                                // is_active
	)
	.fetch_one(pool)
	.await
	.context("Failed to insert delegation into database")?;

	Ok(row.id)
}

/// Fetches all active delegations for the specified slot.
///
/// Each returned item is a `SignedDelegation` reconstructed from the stored database row.
/// This function validates that stored public keys and signatures have the expected byte lengths
/// and will return an error if any row contains malformed byte arrays.
///
/// # Examples
///
/// ```ignore
/// # use sqlx::PgPool;
/// # use anyhow::Result;
/// # async fn example(pool: &PgPool) -> Result<()> {
/// let slot = 42;
/// let delegations = crate::db::delegation_ops::get_delegations_for_slot(pool, slot).await?;
/// println!("Found {} delegations for slot {}", delegations.len(), slot);
/// # Ok(())
/// # }
/// ```ignore
pub async fn get_delegations_for_slot(pool: &PgPool, slot: u64) -> Result<Vec<SignedDelegation>> {
	let rows = sqlx::query!(
		r#"
		SELECT
			proposer_pubkey,
			delegate_pubkey,
			committer_address,
			slot_number,
			signature
		FROM delegations
		WHERE slot_number = $1 AND is_active = true
		"#,
		slot as i64
	)
	.fetch_all(pool)
	.await
	.context("Failed to query delegations for slot")?;

	let mut delegations = Vec::new();

	for row in rows {
		// Convert database bytes back to fixed arrays
		let mut proposer_pubkey = [0u8; 48];
		let mut delegate_pubkey = [0u8; 48];
		let mut signature = [0u8; 96];

		if row.proposer_pubkey.len() != 48 {
			anyhow::bail!("Invalid proposer pubkey length in database: {}", row.proposer_pubkey.len());
		}
		if row.delegate_pubkey.len() != 48 {
			anyhow::bail!("Invalid delegate pubkey length in database: {}", row.delegate_pubkey.len());
		}
		if row.signature.len() != 96 {
			anyhow::bail!("Invalid signature length in database: {}", row.signature.len());
		}

		proposer_pubkey.copy_from_slice(&row.proposer_pubkey);
		delegate_pubkey.copy_from_slice(&row.delegate_pubkey);
		signature.copy_from_slice(&row.signature);

		let delegation_message = DelegationMessage {
			proposer: BlsPublicKey(proposer_pubkey),
			delegate: BlsPublicKey(delegate_pubkey),
			committer: row.committer_address,
			slot: row.slot_number as u64,
		};

		let signed_delegation = SignedDelegation { message: delegation_message, signature: BlsSignature(signature) };

		delegations.push(signed_delegation);
	}

	Ok(delegations)
}

/// Retrieve the active delegation created by a proposer for the specified slot.
///
/// Returns `Some(SignedDelegation)` if an active delegation by `proposer_pubkey` exists for `slot`, `None` otherwise.
///
/// # Examples
///
/// ```ignore
/// # use sqlx::PgPool;
/// # async fn example(pool: &PgPool, proposer: BlsPublicKey) -> anyhow::Result<()> {
/// let maybe = get_delegation_by_proposer_slot(pool, &proposer, 42).await?;
/// if let Some(delegation) = maybe {
///     // inspect delegation.message and delegation.signature
/// }
/// # Ok(())
/// # }
/// ```ignore
pub async fn get_delegation_by_proposer_slot(
	pool: &PgPool,
	proposer_pubkey: &BlsPublicKey,
	slot: u64,
) -> Result<Option<SignedDelegation>> {
	let row = sqlx::query!(
		r#"
		SELECT
			proposer_pubkey,
			delegate_pubkey,
			committer_address,
			slot_number,
			signature
		FROM delegations
		WHERE proposer_pubkey = $1 AND slot_number = $2 AND is_active = true
		"#,
		&proposer_pubkey.0[..], // Convert BlsPublicKey to &[u8]
		slot as i64
	)
	.fetch_optional(pool)
	.await
	.context("Failed to query delegation by proposer and slot")?;

	match row {
		Some(row) => {
			let mut proposer_bytes = [0u8; 48];
			let mut delegate_bytes = [0u8; 48];
			let mut signature_bytes = [0u8; 96];

			proposer_bytes.copy_from_slice(&row.proposer_pubkey);
			delegate_bytes.copy_from_slice(&row.delegate_pubkey);
			signature_bytes.copy_from_slice(&row.signature);

			let delegation_message = DelegationMessage {
				proposer: BlsPublicKey(proposer_bytes),
				delegate: BlsPublicKey(delegate_bytes),
				committer: row.committer_address,
				slot: row.slot_number as u64,
			};

			let signed_delegation =
				SignedDelegation { message: delegation_message, signature: BlsSignature(signature_bytes) };

			Ok(Some(signed_delegation))
		}
		None => Ok(None),
	}
}

/// Fetches all active delegations assigned to a given delegate public key, ordered by slot ascending.
///
/// Returns a vector of `SignedDelegation` objects representing active delegations for the provided
/// delegate public key. The results are ordered by `slot_number` in ascending order.
///
/// # Examples
///
/// ```ignore
/// // Construct or obtain a `PgPool` as `pool` and a `BlsPublicKey` as `delegate_pk`.
/// // This example shows the call pattern; adapt pool creation to your runtime/test setup.
/// # use gateway_domain::{BlsPublicKey, SignedDelegation};
/// # use sqlx::PgPool;
/// # async fn example(pool: &PgPool, delegate_pk: &BlsPublicKey) {
/// let delegations: Vec<SignedDelegation> = get_delegations_by_delegate(pool, delegate_pk).await.unwrap();
/// // `delegations` now contains all active delegations for `delegate_pk` ordered by slot.
/// # }
/// ```ignore
pub async fn get_delegations_by_delegate(
	pool: &PgPool,
	delegate_pubkey: &BlsPublicKey,
) -> Result<Vec<SignedDelegation>> {
	let rows = sqlx::query!(
		r#"
		SELECT
			proposer_pubkey,
			delegate_pubkey,
			committer_address,
			slot_number,
			signature
		FROM delegations
		WHERE delegate_pubkey = $1 AND is_active = true
		ORDER BY slot_number ASC
		"#,
		&delegate_pubkey.0[..]
	)
	.fetch_all(pool)
	.await
	.context("Failed to query delegations by delegate")?;

	let mut delegations = Vec::new();

	for row in rows {
		let mut proposer_bytes = [0u8; 48];
		let mut delegate_bytes = [0u8; 48];
		let mut signature_bytes = [0u8; 96];

		proposer_bytes.copy_from_slice(&row.proposer_pubkey);
		delegate_bytes.copy_from_slice(&row.delegate_pubkey);
		signature_bytes.copy_from_slice(&row.signature);

		let delegation_message = DelegationMessage {
			proposer: BlsPublicKey(proposer_bytes),
			delegate: BlsPublicKey(delegate_bytes),
			committer: row.committer_address,
			slot: row.slot_number as u64,
		};

		let signed_delegation =
			SignedDelegation { message: delegation_message, signature: BlsSignature(signature_bytes) };

		delegations.push(signed_delegation);
	}

	Ok(delegations)
}

/// Check whether an active delegation exists for a given slot and committer address.
///
/// The committer address comparison is performed case-insensitively.
///
/// # Examples
///
/// ```ignore
/// # use sqlx::PgPool;
/// # async fn example(pool: &PgPool) -> anyhow::Result<()> {
/// let exists = delegation_exists_for_slot_and_committer(pool, 42, "0xAbC123...").await?;
/// println!("delegation exists: {}", exists);
/// # Ok(())
/// # }
/// ```ignore
///
/// # Returns
///
/// `true` if an active delegation exists for the specified slot and committer address, `false` otherwise.
pub async fn delegation_exists_for_slot_and_committer(
	pool: &PgPool,
	slot: u64,
	committer_address: &str,
) -> Result<bool> {
	let row = sqlx::query!(
		r#"
		SELECT EXISTS(
			SELECT 1 FROM delegations
			WHERE slot_number = $1 AND LOWER(committer_address) = LOWER($2) AND is_active = true
		) as exists
		"#,
		slot as i64,
		committer_address
	)
	.fetch_one(pool)
	.await
	.context("Failed to check delegation existence")?;

	Ok(row.exists.unwrap_or(false))
}

/// Deactivates delegations whose slot_number is less than `current_slot`.
///
/// Sets `is_active` to `false` for all matching rows and returns the number of rows affected.
///
/// # Parameters
///
/// - `current_slot`: Delegations with `slot_number` less than this value will be deactivated.
///
/// # Returns
///
/// The number of rows that were marked inactive.
///
/// # Examples
///
/// ```ignore
/// # async fn run_example() -> Result<(), Box<dyn std::error::Error>> {
/// // Assume `pool` is a configured `PgPool`.
/// let affected = crate::db::delegation_ops::deactivate_expired_delegations(&pool, 42).await?;
/// assert!(affected >= 0);
/// # Ok(()) }
/// ```ignore
pub async fn deactivate_expired_delegations(pool: &PgPool, current_slot: u64) -> Result<u64> {
	let result = sqlx::query!(
		r#"
		UPDATE delegations
		SET is_active = false
		WHERE slot_number < $1 AND is_active = true
		"#,
		current_slot as i64
	)
	.execute(pool)
	.await
	.context("Failed to deactivate expired delegations")?;

	Ok(result.rows_affected())
}

/// Get delegation statistics for monitoring
#[derive(Debug)]
pub struct DelegationStats {
	pub total_count: i64,
	pub active_count: i64,
	pub unique_proposers: i64,
	pub unique_delegates: i64,
	pub slots_covered: i64,
	pub latest_slot: Option<i64>,
}

/// Returns aggregated statistics about delegations stored in the database.
///
/// The returned `DelegationStats` contains total and active counts, counts of unique
/// proposers and delegates, the number of distinct active slots covered, and the
/// highest active slot number (or `None` if there are no active delegations).
///
/// # Examples
///
/// ```ignorerust,no_run
/// use sqlx::PgPool;
/// // `pool` should be an established PgPool connected to your database.
/// # async fn doc_example(pool: &PgPool) -> Result<(), sqlx::Error> {
/// let stats = crate::db::delegation_ops::get_delegation_stats(pool).await?;
/// println!("Total delegations: {}", stats.total_count);
/// # Ok(())
/// # }
/// ```ignore
pub async fn get_delegation_stats(pool: &PgPool) -> Result<DelegationStats> {
	let row = sqlx::query!(
		r#"
		SELECT
			COUNT(*) as total_count,
			COUNT(*) FILTER (WHERE is_active = true) as active_count,
			COUNT(DISTINCT proposer_pubkey) as unique_proposers,
			COUNT(DISTINCT delegate_pubkey) as unique_delegates,
			COUNT(DISTINCT slot_number) FILTER (WHERE is_active = true) as slots_covered,
			MAX(slot_number) FILTER (WHERE is_active = true) as latest_slot
		FROM delegations
		"#
	)
	.fetch_one(pool)
	.await
	.context("Failed to get delegation statistics")?;

	Ok(DelegationStats {
		total_count: row.total_count.unwrap_or(0),
		active_count: row.active_count.unwrap_or(0),
		unique_proposers: row.unique_proposers.unwrap_or(0),
		unique_delegates: row.unique_delegates.unwrap_or(0),
		slots_covered: row.slots_covered.unwrap_or(0),
		latest_slot: row.latest_slot,
	})
}

/// Saves multiple delegations in a single transaction, returning the IDs of newly inserted rows.
///
/// This performs a batch insert and skips any delegations that conflict on (proposer_pubkey, slot_number),
/// returning only the UUIDs of rows that were actually created. The operation is atomic: either all inserts
/// that can be applied are committed, or the transaction is rolled back on error.
///
/// # Returns
///
/// A `Vec<Uuid>` containing the IDs of delegations that were inserted (duplicates are not included).
///
/// # Examples
///
/// ```ignore
/// # use uuid::Uuid;
/// # use sqlx::PgPool;
/// # use crate::types::{SignedDelegation, DelegationMessage, BlsPublicKey, BlsSignature};
/// # async fn example(pool: &PgPool) -> Result<(), sqlx::Error> {
/// let delegations: Vec<SignedDelegation> = vec![]; // build a few SignedDelegation values
/// let inserted_ids = crate::db::delegation_ops::save_delegations_batch(pool, &delegations).await?;
/// for id in inserted_ids {
///     let _ : Uuid = id;
/// }
/// # Ok(())
/// # }
/// ```ignore
pub async fn save_delegations_batch(pool: &PgPool, delegations: &[SignedDelegation]) -> Result<Vec<Uuid>> {
	let mut ids = Vec::new();

	// Use a transaction for batch operations
	let mut tx = pool.begin().await.context("Failed to begin transaction")?;

	for delegation in delegations {
		let id = Uuid::new_v4();
		let message = &delegation.message;

		// Use ON CONFLICT DO NOTHING to handle duplicates gracefully
		let row = sqlx::query!(
			r#"
			INSERT INTO delegations (
				id,
				proposer_pubkey,
				delegate_pubkey,
				committer_address,
				slot_number,
				signature,
				is_active
			)
			VALUES ($1, $2, $3, $4, $5, $6, $7)
			ON CONFLICT (proposer_pubkey, slot_number) DO NOTHING
			RETURNING id
			"#,
			id,
			&message.proposer.0[..],
			&message.delegate.0[..],
			message.committer,
			message.slot as i64,
			&delegation.signature.0[..],
			true
		)
		.fetch_optional(&mut *tx)
		.await
		.context("Failed to insert delegation in batch")?;

		// Only add ID if the row was actually inserted (not a duplicate)
		if let Some(row) = row {
			ids.push(row.id);
		}
	}

	tx.commit().await.context("Failed to commit delegation batch")?;

	Ok(ids)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::delegation::{DelegationMessage, SignedDelegation};

	/// Creates a sample `SignedDelegation` for use in tests.
	///
	/// The returned delegation uses fixture values for proposer, delegate, committer, slot, and signature.
	///
	/// # Examples
	///
	/// ```ignore
	/// let d = create_test_delegation();
	/// assert_eq!(d.message.proposer.0, [1u8; 48]);
	/// assert_eq!(d.message.delegate.0, [2u8; 48]);
	/// assert_eq!(d.message.committer, "0x1234567890123456789012345678901234567890");
	/// assert_eq!(d.message.slot, 12345);
	/// assert_eq!(d.signature.0, [3u8; 96]);
	/// ```ignore
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

	/// Creates a `SignedDelegation` with custom values for testing different scenarios.
	///
	/// Pads the committer address to a valid 42-character Ethereum address format (0x + 40 hex digits).
	fn create_custom_delegation(proposer_byte: u8, delegate_byte: u8, slot: u64, committer: &str) -> SignedDelegation {
		// Ensure committer is a valid 42-character Ethereum address
		let valid_committer = if committer.len() < 42 {
			// Strip 0x prefix if present, pad with zeros, then re-add 0x
			let hex_part = committer.strip_prefix("0x").unwrap_or(committer);
			format!("0x{:0<40}", hex_part)
		} else {
			committer.to_string()
		};

		SignedDelegation {
			message: DelegationMessage {
				proposer: BlsPublicKey([proposer_byte; 48]),
				delegate: BlsPublicKey([delegate_byte; 48]),
				committer: valid_committer,
				slot,
			},
			signature: BlsSignature([proposer_byte.wrapping_add(delegate_byte); 96]),
		}
	}

	/// Helper function to get a test database pool.
	///
	/// Returns `None` if DATABASE_URL is not set, allowing tests to be skipped gracefully.
	async fn get_test_pool() -> Option<PgPool> {
		let database_url = std::env::var("DATABASE_URL").ok()?;
		PgPool::connect(&database_url).await.ok()
	}

	/// Generate a unique slot number for test isolation.
	///
	/// Uses current timestamp with microsecond precision to create unique slots, preventing duplicate
	/// key violations when tests run multiple times against the same database.
	fn unique_test_slot() -> u64 {
		use std::time::{SystemTime, UNIX_EPOCH};
		let micros = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros();
		// Use very high slot numbers (1 trillion+) to avoid conflicts with real slots and other tests
		// Microseconds since epoch gives us uniqueness even for tests running milliseconds apart
		1_000_000_000_000 + (micros % 1_000_000_000_000) as u64
	}

	// ============================================================================
	// Unit Tests (No Database Required)
	// ============================================================================

	#[test]
	fn test_delegation_creation() {
		let delegation = create_test_delegation();
		assert_eq!(delegation.message.slot, 12345);
		assert_eq!(delegation.message.proposer.0, [1u8; 48]);
		assert_eq!(delegation.message.delegate.0, [2u8; 48]);
		assert_eq!(delegation.signature.0, [3u8; 96]);
		assert_eq!(delegation.message.committer, "0x1234567890123456789012345678901234567890");
	}

	#[test]
	fn test_custom_delegation_creation() {
		let delegation = create_custom_delegation(5, 10, 999, "0xabcd");
		assert_eq!(delegation.message.slot, 999);
		assert_eq!(delegation.message.proposer.0[0], 5);
		assert_eq!(delegation.message.delegate.0[0], 10);
		assert_eq!(delegation.message.committer, "0xabcd000000000000000000000000000000000000");
		assert_eq!(delegation.signature.0[0], 15); // 5 + 10
	}

	#[test]
	fn test_bls_public_key_length() {
		let delegation = create_test_delegation();
		assert_eq!(delegation.message.proposer.0.len(), 48);
		assert_eq!(delegation.message.delegate.0.len(), 48);
	}

	#[test]
	fn test_bls_signature_length() {
		let delegation = create_test_delegation();
		assert_eq!(delegation.signature.0.len(), 96);
	}

	#[test]
	fn test_delegation_stats_debug() {
		let stats = DelegationStats {
			total_count: 100,
			active_count: 50,
			unique_proposers: 10,
			unique_delegates: 5,
			slots_covered: 20,
			latest_slot: Some(12345),
		};
		let debug_str = format!("{:?}", stats);
		assert!(debug_str.contains("total_count: 100"));
		assert!(debug_str.contains("active_count: 50"));
	}

	// ============================================================================
	// Integration Tests (Require PostgreSQL)
	// ============================================================================

	/// Test saving a single delegation and retrieving it.
	#[tokio::test]
	#[ignore] // Run with: cargo test --package preconfirmation-gateway --lib -- db::delegation_ops::tests --ignored
	async fn test_save_and_get_delegation() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot = unique_test_slot();
		let delegation = create_custom_delegation(1, 2, slot, "0x1111");
		let result = save_delegation(&pool, &delegation).await;
		assert!(result.is_ok(), "Failed to save delegation: {:?}", result.err());

		let retrieved = get_delegations_for_slot(&pool, slot).await;
		assert!(retrieved.is_ok(), "Failed to retrieve delegations: {:?}", retrieved.err());

		let delegations = retrieved.unwrap();
		assert!(!delegations.is_empty(), "Expected at least one delegation");

		// Find our delegation
		let found = delegations
			.iter()
			.any(|d| d.message.proposer.0[0] == 1 && d.message.delegate.0[0] == 2 && d.message.slot == slot);
		assert!(found, "Could not find saved delegation");
	}

	/// Test querying delegation by proposer and slot.
	#[tokio::test]
	#[ignore]
	async fn test_get_delegation_by_proposer_slot() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot = unique_test_slot();
		let proposer = BlsPublicKey([7u8; 48]);
		let delegation = create_custom_delegation(7, 8, slot, "0x2222");

		save_delegation(&pool, &delegation).await.expect("Failed to save delegation");

		let result = get_delegation_by_proposer_slot(&pool, &proposer, slot).await;
		assert!(result.is_ok(), "Failed to query by proposer/slot: {:?}", result.err());

		let maybe_delegation = result.unwrap();
		assert!(maybe_delegation.is_some(), "Expected to find delegation");

		let found = maybe_delegation.unwrap();
		assert_eq!(found.message.proposer.0, [7u8; 48]);
		assert_eq!(found.message.delegate.0, [8u8; 48]);
		assert_eq!(found.message.slot, slot);
	}

	/// Test querying delegation by proposer and slot when none exists.
	#[tokio::test]
	#[ignore]
	async fn test_get_delegation_by_proposer_slot_not_found() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let proposer = BlsPublicKey([99u8; 48]);
		let result = get_delegation_by_proposer_slot(&pool, &proposer, 99999).await;
		assert!(result.is_ok());

		let maybe_delegation = result.unwrap();
		assert!(maybe_delegation.is_none(), "Expected no delegation");
	}

	/// Test querying delegations by delegate public key.
	#[tokio::test]
	#[ignore]
	async fn test_get_delegations_by_delegate() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let delegate = BlsPublicKey([20u8; 48]);
		let slot1 = unique_test_slot();
		let slot2 = slot1 + 1;
		let slot3 = slot1 + 2;

		// Save multiple delegations with same delegate but different proposers and slots
		let del1 = create_custom_delegation(10, 20, slot1, "0x3333");
		let del2 = create_custom_delegation(11, 20, slot2, "0x3334");
		let del3 = create_custom_delegation(12, 20, slot3, "0x3335");

		save_delegation(&pool, &del1).await.expect("Failed to save delegation 1");
		save_delegation(&pool, &del2).await.expect("Failed to save delegation 2");
		save_delegation(&pool, &del3).await.expect("Failed to save delegation 3");

		let result = get_delegations_by_delegate(&pool, &delegate).await;
		assert!(result.is_ok(), "Failed to query by delegate: {:?}", result.err());

		let delegations = result.unwrap();
		assert!(delegations.len() >= 3, "Expected at least 3 delegations");

		// Verify they are ordered by slot ascending
		let our_delegations: Vec<_> =
			delegations.iter().filter(|d| d.message.delegate.0[0] == 20 && d.message.slot >= slot1).collect();

		for i in 1..our_delegations.len() {
			assert!(
				our_delegations[i - 1].message.slot <= our_delegations[i].message.slot,
				"Delegations not ordered by slot"
			);
		}
	}

	/// Test checking delegation existence for slot and committer.
	#[tokio::test]
	#[ignore]
	async fn test_delegation_exists_for_slot_and_committer() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot = unique_test_slot();
		let delegation = create_custom_delegation(30, 31, slot, "0x4444");
		save_delegation(&pool, &delegation).await.expect("Failed to save delegation");

		// The delegation was saved with padded address "0x4444000000000000000000000000000000000000"
		let padded_address = "0x4444000000000000000000000000000000000000";
		let exists = delegation_exists_for_slot_and_committer(&pool, slot, padded_address).await;
		assert!(exists.is_ok());
		assert!(exists.unwrap(), "Expected delegation to exist");

		let not_exists =
			delegation_exists_for_slot_and_committer(&pool, slot, "0x9999000000000000000000000000000000000000").await;
		assert!(not_exists.is_ok());
		assert!(!not_exists.unwrap(), "Expected delegation not to exist");
	}

	/// Test case-insensitive committer address matching.
	#[tokio::test]
	#[ignore]
	async fn test_delegation_exists_case_insensitive() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot = unique_test_slot();
		let delegation = create_custom_delegation(40, 41, slot, "0xAbCdEf");
		save_delegation(&pool, &delegation).await.expect("Failed to save delegation");

		// The delegation was saved with padded address "0xAbCdEf0000000000000000000000000000000000"
		// Test different case variations - should match case-insensitively
		let exists_lower =
			delegation_exists_for_slot_and_committer(&pool, slot, "0xabcdef0000000000000000000000000000000000").await;
		assert!(exists_lower.is_ok());
		assert!(exists_lower.unwrap(), "Expected case-insensitive match (lowercase)");

		let exists_upper =
			delegation_exists_for_slot_and_committer(&pool, slot, "0xABCDEF0000000000000000000000000000000000").await;
		assert!(exists_upper.is_ok());
		assert!(exists_upper.unwrap(), "Expected case-insensitive match (uppercase)");
	}

	/// Test batch saving with multiple delegations.
	#[tokio::test]
	#[ignore]
	async fn test_save_delegations_batch() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot1 = unique_test_slot();
		let delegations = vec![
			create_custom_delegation(50, 51, slot1, "0x5555"),
			create_custom_delegation(52, 53, slot1 + 1, "0x5556"),
			create_custom_delegation(54, 55, slot1 + 2, "0x5557"),
		];

		let result = save_delegations_batch(&pool, &delegations).await;
		assert!(result.is_ok(), "Failed to save batch: {:?}", result.err());

		let ids = result.unwrap();
		assert_eq!(ids.len(), 3, "Expected 3 IDs from batch insert");

		// Verify all were saved
		for delegation in &delegations {
			let found = get_delegations_for_slot(&pool, delegation.message.slot).await;
			assert!(found.is_ok());
			assert!(!found.unwrap().is_empty());
		}
	}

	/// Test batch saving with duplicate handling.
	#[tokio::test]
	#[ignore]
	async fn test_save_delegations_batch_with_duplicates() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot = unique_test_slot();
		let delegation = create_custom_delegation(60, 61, slot, "0x6666");

		// Save once
		let first_result = save_delegations_batch(&pool, &[delegation.clone()]).await;
		assert!(first_result.is_ok());
		assert_eq!(first_result.unwrap().len(), 1);

		// Save again (should be ignored due to conflict)
		let second_result = save_delegations_batch(&pool, &[delegation.clone()]).await;
		assert!(second_result.is_ok());
		assert_eq!(second_result.unwrap().len(), 0, "Expected duplicate to be ignored");
	}

	/// Test batch saving with partial duplicates.
	#[tokio::test]
	#[ignore]
	async fn test_save_delegations_batch_partial_duplicates() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot1 = unique_test_slot();
		let del1 = create_custom_delegation(70, 71, slot1, "0x7777");
		let del2 = create_custom_delegation(72, 73, slot1 + 1, "0x7778");

		// Save first one
		save_delegation(&pool, &del1).await.expect("Failed to save first delegation");

		// Batch save both (one is duplicate, one is new)
		let result = save_delegations_batch(&pool, &[del1, del2]).await;
		assert!(result.is_ok());

		let ids = result.unwrap();
		assert_eq!(ids.len(), 1, "Expected only 1 new ID (duplicate ignored)");
	}

	/// Test deactivating expired delegations.
	#[tokio::test]
	#[ignore]
	async fn test_deactivate_expired_delegations() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let base_slot = unique_test_slot();
		let old_slot1 = base_slot;
		let old_slot2 = base_slot + 1;
		let new_slot = base_slot + 1000;
		let cutoff_slot = base_slot + 100;

		// Save delegations with various slots
		let old_del1 = create_custom_delegation(80, 81, old_slot1, "0x8888");
		let old_del2 = create_custom_delegation(82, 83, old_slot2, "0x8889");
		let new_del = create_custom_delegation(84, 85, new_slot, "0x8890");

		save_delegation(&pool, &old_del1).await.expect("Failed to save old delegation 1");
		save_delegation(&pool, &old_del2).await.expect("Failed to save old delegation 2");
		save_delegation(&pool, &new_del).await.expect("Failed to save new delegation");

		// Deactivate delegations before cutoff_slot
		let result = deactivate_expired_delegations(&pool, cutoff_slot).await;
		assert!(result.is_ok(), "Failed to deactivate: {:?}", result.err());

		let affected = result.unwrap();
		assert!(affected >= 2, "Expected at least 2 rows affected, got {}", affected);

		// Verify old delegations are not returned in active queries
		let slot_50_delegations = get_delegations_for_slot(&pool, old_slot1).await;
		assert!(slot_50_delegations.is_ok());

		let delegations_50 = slot_50_delegations.unwrap();
		let active_50 = delegations_50.iter().filter(|d| d.message.proposer.0[0] == 80).collect::<Vec<_>>();
		assert!(active_50.is_empty(), "Expected no active delegations for expired slot");

		// Verify new delegation is still active
		let slot_1000_delegations = get_delegations_for_slot(&pool, new_slot).await;
		assert!(slot_1000_delegations.is_ok());

		let active_1000 = slot_1000_delegations.unwrap();
		let found_active = active_1000.iter().any(|d| d.message.proposer.0[0] == 84 && d.message.slot == new_slot);
		assert!(found_active, "Expected new delegation to still be active");
	}

	/// Test getting delegation statistics.
	#[tokio::test]
	#[ignore]
	async fn test_get_delegation_stats() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		// Get initial stats
		let initial_stats = get_delegation_stats(&pool).await;
		assert!(initial_stats.is_ok());

		let stats = initial_stats.unwrap();
		assert!(stats.total_count >= 0);
		assert!(stats.active_count >= 0);
		assert!(stats.active_count <= stats.total_count);
	}

	/// Test delegation stats with known data.
	#[tokio::test]
	#[ignore]
	async fn test_delegation_stats_with_data() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot1 = unique_test_slot();
		// Save some delegations
		let del1 = create_custom_delegation(90, 91, slot1, "0x9999");
		let del2 = create_custom_delegation(92, 91, slot1 + 1, "0x9999"); // Same delegate
		let del3 = create_custom_delegation(90, 93, slot1 + 2, "0x9998"); // Same proposer, different delegate

		save_delegation(&pool, &del1).await.expect("Failed to save delegation 1");
		save_delegation(&pool, &del2).await.expect("Failed to save delegation 2");
		save_delegation(&pool, &del3).await.expect("Failed to save delegation 3");

		let stats = get_delegation_stats(&pool).await;
		assert!(stats.is_ok());

		let stats = stats.unwrap();
		assert!(stats.active_count >= 3, "Expected at least 3 active delegations");
		assert!(stats.latest_slot.is_some());

		if let Some(latest) = stats.latest_slot {
			assert!(latest >= (slot1 + 2) as i64, "Latest slot should be at least slot1 + 2");
		}
	}

	/// Test querying multiple slots.
	#[tokio::test]
	#[ignore]
	async fn test_get_delegations_multiple_slots() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		let slot1 = unique_test_slot();
		let slot2 = slot1 + 1;
		// Save delegations for different slots
		let del1 = create_custom_delegation(100, 101, slot1, "0xaaaa");
		let del2 = create_custom_delegation(102, 103, slot1, "0xaaab"); // Same slot, different proposer
		let del3 = create_custom_delegation(104, 105, slot2, "0xaaac"); // Different slot

		save_delegation(&pool, &del1).await.expect("Failed to save delegation 1");
		save_delegation(&pool, &del2).await.expect("Failed to save delegation 2");
		save_delegation(&pool, &del3).await.expect("Failed to save delegation 3");

		// Query slot with multiple delegations
		let slot_1200 = get_delegations_for_slot(&pool, slot1).await;
		assert!(slot_1200.is_ok());

		let delegations_1200 = slot_1200.unwrap();
		let our_delegations =
			delegations_1200.iter().filter(|d| d.message.slot == slot1 && d.message.proposer.0[0] >= 100).count();
		assert!(our_delegations >= 2, "Expected at least 2 delegations for slot1");

		// Query slot with single delegation
		let slot_1201 = get_delegations_for_slot(&pool, slot2).await;
		assert!(slot_1201.is_ok());

		let delegations_1201 = slot_1201.unwrap();
		let found_1201 = delegations_1201.iter().any(|d| d.message.proposer.0[0] == 104);
		assert!(found_1201, "Expected to find delegation for slot2");
	}

	/// Test empty result scenarios.
	#[tokio::test]
	#[ignore]
	async fn test_empty_queries() {
		let pool = get_test_pool().await.expect("DATABASE_URL must be set for integration tests");

		// Query non-existent slot
		let result = get_delegations_for_slot(&pool, 999999).await;
		assert!(result.is_ok());
		// Note: May have delegations from other tests, just verify no error

		// Query non-existent delegate
		let delegate = BlsPublicKey([255u8; 48]);
		let result = get_delegations_by_delegate(&pool, &delegate).await;
		assert!(result.is_ok());
		// May be empty or not depending on other tests
	}
}
