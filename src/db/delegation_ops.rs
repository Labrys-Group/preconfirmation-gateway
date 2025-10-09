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

			if row.proposer_pubkey.len() != 48 {
				anyhow::bail!("Invalid proposer pubkey length: {}", row.proposer_pubkey.len());
			}
			if row.delegate_pubkey.len() != 48 {
				anyhow::bail!("Invalid delegate pubkey length: {}", row.delegate_pubkey.len());
			}
			if row.signature.len() != 96 {
				anyhow::bail!("Invalid signature length: {}", row.signature.len());
			}
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

		if row.proposer_pubkey.len() != 48 {
			anyhow::bail!("Invalid proposer pubkey length: {}", row.proposer_pubkey.len());
		}
		if row.delegate_pubkey.len() != 48 {
			anyhow::bail!("Invalid delegate pubkey length: {}", row.delegate_pubkey.len());
		}
		if row.signature.len() != 96 {
			anyhow::bail!("Invalid signature length: {}", row.signature.len());
		}
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

	// These tests would require a test database setup
	// For now, we'll create placeholder tests that verify the function signatures

	/// Creates a sample `SignedDelegation` for use in tests.
	///
	/// The returned delegation uses fixture values for proposer, delegate, committer, slot, and signature.
	///
	/// # Examples
	///
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

	#[test]
	fn test_delegation_creation() {
		let delegation = create_test_delegation();
		assert_eq!(delegation.message.slot, 12345);
		assert_eq!(delegation.message.proposer.0, [1u8; 48]);
		assert_eq!(delegation.message.delegate.0, [2u8; 48]);
		assert_eq!(delegation.signature.0, [3u8; 96]);
	}
}
