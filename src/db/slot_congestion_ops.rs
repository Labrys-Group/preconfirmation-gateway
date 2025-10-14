use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::debug;

/// Slot congestion data for fee calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotCongestion {
	pub id: Option<i32>,
	pub slot: u64,
	pub preconfirmed_gas: u64,
	pub total_gas_limit: u64,
	pub gas_used_ratio: f64,
	pub base_gas_price: u64,
	pub calculated_fee_multiplier: f64,
	pub current_tx_price: u64,
	pub slot_start_time: SystemTime,
	pub last_updated: Option<SystemTime>,
	pub created_at: Option<SystemTime>,
}

impl SlotCongestion {
	/// Creates a new in-memory SlotCongestion for the given slot with initial/default metrics.
	///
	/// The returned struct has zero preconfirmed gas, gas_used_ratio 0.0, calculated_fee_multiplier 1.0,
	/// and current_tx_price initialized to `base_gas_price`.
	///
	/// # Parameters
	///
	/// - `slot`: Slot number to track.
	/// - `base_gas_price`: Base gas price in wei used as the initial current_tx_price.
	/// - `total_gas_limit`: Gas limit for the slot used to compute congestion ratios.
	/// - `slot_start_time`: Start time of the slot.
	///
	/// # Examples
	///
	pub fn new(slot: u64, base_gas_price: u64, total_gas_limit: u64, slot_start_time: SystemTime) -> Self {
		Self {
			id: None,
			slot,
			preconfirmed_gas: 0,
			total_gas_limit,
			gas_used_ratio: 0.0,
			base_gas_price,
			calculated_fee_multiplier: 1.0,
			current_tx_price: base_gas_price,
			slot_start_time,
			last_updated: None,
			created_at: None,
		}
	}

	/// Apply additional gas usage to this SlotCongestion and update its derived congestion metrics.
	///
	/// This method increments `preconfirmed_gas`, recomputes `gas_used_ratio`, derives a congestion
	/// multiplier using the formula `multiplier = 1 / (1 - (gas_used_ratio)^k)`, clamps the multiplier
	/// to the range [1.0, 100.0], and updates `current_tx_price` = `base_gas_price * multiplier`.
	///
	/// The `scaling_factor` (k) controls how sharply the multiplier grows as the slot fills.
	///
	/// # Examples
	///
	pub fn add_gas_usage(&mut self, additional_gas: u64, scaling_factor: f64) {
		// Use saturating_add to prevent overflow when accumulating gas
		self.preconfirmed_gas = self.preconfirmed_gas.saturating_add(additional_gas);

		// Guard against division by zero - if total_gas_limit is 0, set max multiplier
		if self.total_gas_limit == 0 {
			self.gas_used_ratio = 0.0;
			self.calculated_fee_multiplier = 100.0;
			self.current_tx_price = self.base_gas_price.saturating_mul(100);
			return;
		}

		// Calculate gas usage ratio with safe division (already checked total_gas_limit > 0)
		self.gas_used_ratio = self.preconfirmed_gas as f64 / self.total_gas_limit as f64;

		// Clamp ratio to valid range [0.0, f64::MAX] to handle overflow cases
		// Note: We don't clamp to 1.0 here because we want to track over-subscription
		self.gas_used_ratio = self.gas_used_ratio.max(0.0);

		// Guard against NaN from the division (shouldn't happen after the zero check, but be defensive)
		if !self.gas_used_ratio.is_finite() {
			self.gas_used_ratio = 0.0;
			self.calculated_fee_multiplier = 100.0;
			self.current_tx_price = self.base_gas_price.saturating_mul(100);
			return;
		}

		// Calculate fee multiplier using the congestion formula
		// multiplier = 1 / (1 - (gas_ratio)^k)
		if self.gas_used_ratio >= 1.0 {
			// Prevent division by zero - at 100% usage, use max multiplier
			self.calculated_fee_multiplier = 100.0;
		} else {
			let ratio_powered = self.gas_used_ratio.powf(scaling_factor);
			self.calculated_fee_multiplier = 1.0 / (1.0 - ratio_powered);
		}

		// Validate fee multiplier is finite before clamping
		if !self.calculated_fee_multiplier.is_finite() {
			self.calculated_fee_multiplier = 100.0;
		} else {
			// Apply bounds checking
			self.calculated_fee_multiplier = self.calculated_fee_multiplier.clamp(1.0, 100.0);
		}

		// Calculate final transaction price (round up to match calculate_fee_for_commitment)
		let scaled_price = self.base_gas_price as f64 * self.calculated_fee_multiplier;

		// Ensure the scaled price is finite before casting to u64
		if !scaled_price.is_finite() || scaled_price < 0.0 {
			// Fallback to max allowed price to avoid undercharging
			self.current_tx_price = self.base_gas_price.saturating_mul(100);
		} else {
			self.current_tx_price = scaled_price.ceil() as u64;
		}
	}
}

/// Ensure a slot congestion record exists and return its in-memory representation.
///
/// If no row exists for `slot`, this function inserts a new row initialized from the
/// provided parameters and the computed slot start time; if a concurrent insert wins,
/// the existing row is fetched and returned.
///
/// # Parameters
///
/// - `genesis_time`: UNIX timestamp in seconds for chain genesis; used to compute the slot's start time.
///
/// # Returns
///
/// A `SlotCongestion` for the given `slot`, reflecting either the newly created database row or the existing one.
///
/// # Examples
///
pub async fn get_or_create_slot_congestion(
	pool: &PgPool,
	slot: u64,
	base_gas_price: u64,
	total_gas_limit: u64,
	genesis_time: u64,
) -> Result<SlotCongestion> {
	// Calculate slot start time with overflow protection
	let slot_start_timestamp = genesis_time.saturating_add(slot.saturating_mul(12)); // 12-second slots
	let slot_start_time = UNIX_EPOCH + Duration::from_secs(slot_start_timestamp);

	// Create new record struct (in memory)
	let congestion = SlotCongestion::new(slot, base_gas_price, total_gas_limit, slot_start_time);

	let slot_start_time_chrono = sqlx::types::chrono::DateTime::from_timestamp(slot_start_timestamp as i64, 0)
		.ok_or_else(|| anyhow::anyhow!("Invalid slot start timestamp"))?
		.naive_utc();

	// Try to insert, but do nothing if a row for this slot already exists
	let insert_result = sqlx::query!(
		r#"
        INSERT INTO slot_congestion (
            slot, preconfirmed_gas, total_gas_limit, gas_used_ratio,
            base_gas_price, calculated_fee_multiplier, current_tx_price, slot_start_time
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (slot) DO NOTHING
        RETURNING id
        "#,
		slot as i64,
		congestion.preconfirmed_gas as i64,
		congestion.total_gas_limit as i64,
		congestion.gas_used_ratio,
		congestion.base_gas_price as i64,
		congestion.calculated_fee_multiplier,
		congestion.current_tx_price as i64,
		slot_start_time_chrono
	)
	.fetch_optional(pool)
	.await
	.context("Failed to insert slot congestion record")?;

	// If we got an id back, we won the race
	if let Some(id_row) = insert_result {
		debug!("Created new slot congestion record for slot {} with ID {}", slot, id_row.id);
		let mut result = congestion;
		result.id = Some(id_row.id);
		return Ok(result);
	}

	// Otherwise another task won the race—re-fetch the existing record
	get_slot_congestion(pool, slot)
		.await?
		.ok_or_else(|| anyhow::anyhow!("Slot congestion unexpectedly missing after insert for slot {}", slot))
}

/// Fetches congestion metrics for the specified slot from the database.
///
/// # Returns
///
/// `Ok(Some(SlotCongestion))` with the stored metrics when a row for the slot exists, `Ok(None)` when no record is found, or an error if the database query fails.
///
/// # Examples
///
pub async fn get_slot_congestion(pool: &PgPool, slot: u64) -> Result<Option<SlotCongestion>> {
	let row = sqlx::query!(
		r#"
        SELECT
            id, slot, preconfirmed_gas, total_gas_limit,
            gas_used_ratio, base_gas_price, calculated_fee_multiplier, current_tx_price,
            slot_start_time, last_updated, created_at
        FROM slot_congestion
        WHERE slot = $1
        "#,
		slot as i64
	)
	.fetch_optional(pool)
	.await
	.context("Failed to query slot congestion")?;

	if let Some(row) = row {
		let slot_congestion = SlotCongestion {
			id: Some(row.id),
			slot: row.slot as u64,
			preconfirmed_gas: row.preconfirmed_gas as u64,
			total_gas_limit: row.total_gas_limit as u64,
			gas_used_ratio: row.gas_used_ratio,
			base_gas_price: row.base_gas_price as u64,
			calculated_fee_multiplier: row.calculated_fee_multiplier,
			current_tx_price: row.current_tx_price as u64,
			slot_start_time: UNIX_EPOCH + Duration::from_secs(row.slot_start_time.and_utc().timestamp() as u64),
			last_updated: row.last_updated.map(|dt| UNIX_EPOCH + Duration::from_secs(dt.and_utc().timestamp() as u64)),
			created_at: row.created_at.map(|dt| UNIX_EPOCH + Duration::from_secs(dt.and_utc().timestamp() as u64)),
		};

		Ok(Some(slot_congestion))
	} else {
		Ok(None)
	}
}

/// Update the stored congestion metrics for a specific slot by applying additional gas usage.
///
/// This locks the row for the given `slot`, applies `additional_gas` using the provided
/// `scaling_factor` to recompute the gas usage ratio, fee multiplier, and current transaction price,
/// persists those updated fields and returns the updated `SlotCongestion`.
///
/// # Parameters
///
/// - `pool`: Postgres connection pool used to perform the update.
/// - `slot`: The slot number whose congestion record should be updated.
/// - `additional_gas`: Additional gas to add to the slot's preconfirmed gas tally.
/// - `scaling_factor`: Exponent used when computing the congestion-based fee multiplier.
///
/// # Returns
///
/// The updated `SlotCongestion` reflecting the persisted changes.
///
/// # Examples
///
pub async fn update_slot_congestion_gas_usage(
	pool: &PgPool,
	slot: u64,
	additional_gas: u64,
	scaling_factor: f64,
) -> Result<SlotCongestion> {
	let mut tx = pool.begin().await.context("Failed to begin slot congestion update transaction")?;

	let row = sqlx::query!(
		r#"
        SELECT
            id, slot, preconfirmed_gas, total_gas_limit,
            gas_used_ratio, base_gas_price, calculated_fee_multiplier, current_tx_price,
            slot_start_time, last_updated, created_at
        FROM slot_congestion
        WHERE slot = $1
        FOR UPDATE
        "#,
		slot as i64
	)
	.fetch_optional(&mut *tx)
	.await
	.context("Failed to lock slot congestion row")?
	.ok_or_else(|| anyhow::anyhow!("Slot congestion record not found for slot {}", slot))?;

	let mut congestion = SlotCongestion {
		id: Some(row.id),
		slot: row.slot as u64,
		preconfirmed_gas: row.preconfirmed_gas as u64,
		total_gas_limit: row.total_gas_limit as u64,
		gas_used_ratio: row.gas_used_ratio,
		base_gas_price: row.base_gas_price as u64,
		calculated_fee_multiplier: row.calculated_fee_multiplier,
		current_tx_price: row.current_tx_price as u64,
		slot_start_time: UNIX_EPOCH + Duration::from_secs(row.slot_start_time.and_utc().timestamp() as u64),
		last_updated: row.last_updated.map(|dt| UNIX_EPOCH + Duration::from_secs(dt.and_utc().timestamp() as u64)),
		created_at: row.created_at.map(|dt| UNIX_EPOCH + Duration::from_secs(dt.and_utc().timestamp() as u64)),
	};

	congestion.add_gas_usage(additional_gas, scaling_factor);

	sqlx::query!(
		r#"
        UPDATE slot_congestion SET
            preconfirmed_gas = $2,
            gas_used_ratio = $3,
            calculated_fee_multiplier = $4,
            current_tx_price = $5,
            last_updated = NOW()
        WHERE slot = $1
        "#,
		slot as i64,
		congestion.preconfirmed_gas as i64,
		congestion.gas_used_ratio,
		congestion.calculated_fee_multiplier,
		congestion.current_tx_price as i64
	)
	.execute(&mut *tx)
	.await
	.context("Failed to update slot congestion")?;

	tx.commit().await.context("Failed to commit slot congestion update transaction")?;

	debug!(
		"Updated slot {} congestion: {}% full, {:.2}x multiplier, {} wei price",
		slot,
		(congestion.gas_used_ratio * 100.0),
		congestion.calculated_fee_multiplier,
		congestion.current_tx_price
	);

	Ok(congestion)
}

/// Returns the current transaction price for the given slot, adjusted for congestion.
///
/// # Returns
/// `Some(price)` with the price in wei if the slot exists, `None` if no record is present.
///
/// # Examples
///
pub async fn get_current_gas_price_for_slot(pool: &PgPool, slot: u64) -> Result<Option<u64>> {
	let row = sqlx::query!("SELECT current_tx_price FROM slot_congestion WHERE slot = $1", slot as i64)
		.fetch_optional(pool)
		.await
		.context("Failed to query current gas price for slot")?;

	Ok(row.map(|r| r.current_tx_price as u64))
}

/// Removes slot congestion rows whose slot_start_time is older than the given retention window.
///
/// Deletes records with slot_start_time earlier than now minus `hours_to_keep` hours and returns
/// the number of rows removed.
///
/// # Parameters
///
/// - `hours_to_keep`: retention window in hours; records older than this will be deleted.
///
/// # Returns
///
/// The number of rows deleted as `u64`.
///
/// # Examples
///
pub async fn cleanup_old_slot_congestion(pool: &PgPool, hours_to_keep: u32) -> Result<u64> {
	// Use saturating_mul to prevent overflow when computing duration
	let cutoff_duration_secs = (hours_to_keep as u64).saturating_mul(3600);
	let cutoff_time = SystemTime::now() - Duration::from_secs(cutoff_duration_secs);
	let cutoff_timestamp = cutoff_time.duration_since(UNIX_EPOCH)?.as_secs() as i64;

	let cutoff_chrono = sqlx::types::chrono::DateTime::from_timestamp(cutoff_timestamp, 0)
		.ok_or_else(|| anyhow::anyhow!("Invalid cutoff timestamp"))?
		.naive_utc();

	let result = sqlx::query!("DELETE FROM slot_congestion WHERE slot_start_time < $1", cutoff_chrono)
		.execute(pool)
		.await
		.context("Failed to cleanup old slot congestion records")?;

	let deleted_count = result.rows_affected();
	if deleted_count > 0 {
		debug!("Cleaned up {} old slot congestion records", deleted_count);
	}

	Ok(deleted_count)
}

/// Get congestion statistics for monitoring
#[derive(Debug, Serialize)]
pub struct CongestionStats {
	pub total_slots_tracked: u64,
	pub current_average_congestion: f64,
	pub highest_congestion_slot: Option<u64>,
	pub highest_congestion_ratio: f64,
	pub average_fee_multiplier: f64,
}

/// Compute aggregate congestion statistics for the last 24 hours from the `slot_congestion` table.
///
/// Returns a `CongestionStats` value containing:
/// - `total_slots_tracked`: number of distinct slots tracked in the last 24 hours,
/// - `current_average_congestion`: average `gas_used_ratio` over that window,
/// - `highest_congestion_slot`: `Some(slot)` for the slot with the highest `gas_used_ratio` in the window, or `None` if no congestion was recorded,
/// - `highest_congestion_ratio`: the maximum `gas_used_ratio` observed,
/// - `average_fee_multiplier`: average `calculated_fee_multiplier` over the window.
///
/// # Examples
///
pub async fn get_congestion_stats(pool: &PgPool) -> Result<CongestionStats> {
	let stats = sqlx::query!(
		r#"
        SELECT
            COUNT(*) as "total_slots!",
            COALESCE(AVG(gas_used_ratio), 0.0) as "avg_congestion!",
            COALESCE(AVG(calculated_fee_multiplier), 1.0) as "avg_multiplier!",
            COALESCE(MAX(gas_used_ratio), 0.0) as "max_congestion!"
        FROM slot_congestion
        WHERE slot_start_time > NOW() - INTERVAL '24 hours'
        "#
	)
	.fetch_one(pool)
	.await
	.context("Failed to get congestion statistics")?;

	let highest_congestion_slot = if stats.max_congestion > 0.0 {
		let row = sqlx::query!(
			r#"
            SELECT slot, gas_used_ratio
            FROM slot_congestion
            WHERE slot_start_time > NOW() - INTERVAL '24 hours'
            ORDER BY gas_used_ratio DESC
            LIMIT 1
            "#
		)
		.fetch_optional(pool)
		.await
		.context("Failed to find highest congestion slot")?;

		row.map(|r| r.slot as u64)
	} else {
		None
	};

	Ok(CongestionStats {
		total_slots_tracked: stats.total_slots as u64,
		current_average_congestion: stats.avg_congestion,
		highest_congestion_slot,
		highest_congestion_ratio: stats.max_congestion,
		average_fee_multiplier: stats.avg_multiplier,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use sqlx::PgPool;

	#[test]
	fn test_slot_congestion_creation() {
		let slot = 12345;
		let base_price = 1_000_000_000; // 1 gwei
		let gas_limit = 30_000_000;
		let start_time = SystemTime::now();

		let congestion = SlotCongestion::new(slot, base_price, gas_limit, start_time);

		assert_eq!(congestion.slot, slot);
		assert_eq!(congestion.base_gas_price, base_price);
		assert_eq!(congestion.preconfirmed_gas, 0);
		assert_eq!(congestion.gas_used_ratio, 0.0);
		assert_eq!(congestion.calculated_fee_multiplier, 1.0);
		assert_eq!(congestion.current_tx_price, base_price);
	}

	#[test]
	fn test_gas_usage_calculation() {
		let mut congestion = SlotCongestion::new(
			12345,
			1_000_000_000, // 1 gwei base price
			30_000_000,    // 30M gas limit
			SystemTime::now(),
		);

		// Add 15M gas (50% of limit) with scaling factor k=2
		congestion.add_gas_usage(15_000_000, 2.0);

		assert_eq!(congestion.preconfirmed_gas, 15_000_000);
		assert_eq!(congestion.gas_used_ratio, 0.5);

		// With 50% usage and k=2: multiplier = 1 / (1 - 0.5^2) = 1 / (1 - 0.25) = 1.333...
		assert!((congestion.calculated_fee_multiplier - 1.333).abs() < 0.01);

		// Price should be base_price * multiplier, rounded up
		let expected_price = (1_000_000_000.0 * congestion.calculated_fee_multiplier).ceil() as u64;
		assert_eq!(congestion.current_tx_price, expected_price);
	}

	#[test]
	fn test_high_congestion_bounds() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add 29M gas (96.7% of limit)
		congestion.add_gas_usage(29_000_000, 2.0);

		// Should be bounded to reasonable maximum
		assert!(congestion.calculated_fee_multiplier >= 1.0);
		assert!(congestion.calculated_fee_multiplier <= 100.0);
		assert!(congestion.current_tx_price >= congestion.base_gas_price);
	}

	#[test]
	fn test_full_congestion() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add full gas limit
		congestion.add_gas_usage(30_000_000, 2.0);

		assert_eq!(congestion.gas_used_ratio, 1.0);
		assert_eq!(congestion.calculated_fee_multiplier, 100.0); // Max multiplier
		assert_eq!(congestion.current_tx_price, 100_000_000_000); // 100x base price
	}

	#[test]
	fn test_zero_gas_usage() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add zero gas
		congestion.add_gas_usage(0, 2.0);

		assert_eq!(congestion.preconfirmed_gas, 0);
		assert_eq!(congestion.gas_used_ratio, 0.0);
		assert_eq!(congestion.calculated_fee_multiplier, 1.0);
		assert_eq!(congestion.current_tx_price, congestion.base_gas_price);
	}

	#[test]
	fn test_small_gas_usage() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add 1% of gas limit
		congestion.add_gas_usage(300_000, 2.0);

		assert_eq!(congestion.preconfirmed_gas, 300_000);
		assert_eq!(congestion.gas_used_ratio, 0.01);
		assert!(congestion.calculated_fee_multiplier > 1.0);
		assert!(congestion.calculated_fee_multiplier < 1.1); // Should be close to 1.0
	}

	#[test]
	fn test_multiple_gas_additions() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add gas in multiple steps
		congestion.add_gas_usage(10_000_000, 2.0);
		assert_eq!(congestion.preconfirmed_gas, 10_000_000);
		assert_eq!(congestion.gas_used_ratio, 10_000_000.0 / 30_000_000.0);

		congestion.add_gas_usage(5_000_000, 2.0);
		assert_eq!(congestion.preconfirmed_gas, 15_000_000);
		assert_eq!(congestion.gas_used_ratio, 0.5);
	}

	#[test]
	fn test_different_scaling_factors() {
		let mut congestion1 = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());
		let mut congestion2 = SlotCongestion::new(12346, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add same gas with different scaling factors
		congestion1.add_gas_usage(15_000_000, 1.0); // Linear scaling
		congestion2.add_gas_usage(15_000_000, 3.0); // Cubic scaling

		assert_eq!(congestion1.gas_used_ratio, congestion2.gas_used_ratio);
		// Higher scaling factor results in lower fee multiplier for same gas usage
		assert!(congestion2.calculated_fee_multiplier < congestion1.calculated_fee_multiplier);
	}

	#[test]
	fn test_overflow_protection() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add more gas than the limit
		congestion.add_gas_usage(50_000_000, 2.0);

		// Gas ratio can exceed 1.0, but fee multiplier is capped at 100.0
		assert!(congestion.gas_used_ratio > 1.0);
		assert_eq!(congestion.calculated_fee_multiplier, 100.0); // Max multiplier
	}

	#[test]
	fn test_congestion_stats_creation() {
		let stats = CongestionStats {
			total_slots_tracked: 100,
			current_average_congestion: 0.5,
			highest_congestion_slot: Some(12345),
			highest_congestion_ratio: 0.95,
			average_fee_multiplier: 2.5,
		};

		assert_eq!(stats.total_slots_tracked, 100);
		assert_eq!(stats.current_average_congestion, 0.5);
		assert_eq!(stats.highest_congestion_slot, Some(12345));
		assert_eq!(stats.highest_congestion_ratio, 0.95);
		assert_eq!(stats.average_fee_multiplier, 2.5);
	}

	#[test]
	fn test_congestion_stats_empty() {
		let stats = CongestionStats {
			total_slots_tracked: 0,
			current_average_congestion: 0.0,
			highest_congestion_slot: None,
			highest_congestion_ratio: 0.0,
			average_fee_multiplier: 1.0,
		};

		assert_eq!(stats.total_slots_tracked, 0);
		assert_eq!(stats.highest_congestion_slot, None);
	}

	#[tokio::test]
	async fn test_get_or_create_slot_congestion_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_or_create_slot_congestion(&invalid_pool, 12345, 1_000_000_000, 30_000_000, 1606824023).await;

		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_slot_congestion_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_slot_congestion(&invalid_pool, 12345).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_update_slot_congestion_gas_usage_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = update_slot_congestion_gas_usage(&invalid_pool, 12345, 1_000_000, 2.0).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_current_gas_price_for_slot_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_current_gas_price_for_slot(&invalid_pool, 12345).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_cleanup_old_slot_congestion_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = cleanup_old_slot_congestion(&invalid_pool, 24).await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_get_congestion_stats_with_invalid_pool() {
		let invalid_pool = PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid_db").unwrap();

		let result = get_congestion_stats(&invalid_pool).await;
		assert!(result.is_err());
	}

	#[test]
	fn test_slot_start_time_calculation() {
		let genesis_time = 1606824023; // Ethereum mainnet genesis
		let slot = 1000;
		let expected_timestamp = genesis_time + (slot * 12); // 12-second slots

		let slot_start_time = UNIX_EPOCH + Duration::from_secs(expected_timestamp);

		// Test that the calculation is correct
		assert_eq!(slot_start_time.duration_since(UNIX_EPOCH).unwrap().as_secs(), expected_timestamp);
	}

	#[test]
	fn test_timestamp_overflow_protection() {
		// Test that saturating arithmetic prevents overflow in timestamp calculations
		let genesis_time = u64::MAX - 100; // Near maximum value
		let slot = u64::MAX; // Maximum slot value

		// This would overflow with plain addition, but saturates to u64::MAX
		let slot_start_timestamp = genesis_time.saturating_add(slot.saturating_mul(12));
		assert_eq!(slot_start_timestamp, u64::MAX);

		// Test cleanup duration calculation that WILL overflow
		// u64::MAX / 3600 ≈ 5,124,095,576,030,430
		// Use a larger value to force overflow
		let hours_that_overflow = u64::MAX / 3600 + 1;
		let cutoff_duration_secs = hours_that_overflow.saturating_mul(3600);
		// With saturation, this caps at u64::MAX instead of wrapping around
		assert_eq!(cutoff_duration_secs, u64::MAX);
	}

	#[test]
	fn test_multiple_saturating_gas_additions() {
		// Test that multiple saturating additions work correctly
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Start near max value
		congestion.preconfirmed_gas = u64::MAX - 1000;

		// Add more gas - should saturate at u64::MAX
		congestion.add_gas_usage(5000, 2.0);
		assert_eq!(congestion.preconfirmed_gas, u64::MAX);

		// Verify the fee multiplier is still calculated correctly (max multiplier)
		assert_eq!(congestion.calculated_fee_multiplier, 100.0);
		assert!(congestion.gas_used_ratio.is_finite());
	}

	#[test]
	fn test_fee_multiplier_formula_edge_cases() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Test very small usage (should be close to 1.0)
		congestion.add_gas_usage(1, 2.0);
		assert!(congestion.calculated_fee_multiplier >= 1.0);
		assert!(congestion.calculated_fee_multiplier < 1.01);

		// Reset and test 99% usage
		let mut congestion2 = SlotCongestion::new(12346, 1_000_000_000, 30_000_000, SystemTime::now());
		congestion2.add_gas_usage(29_700_000, 2.0); // 99%
		assert!(congestion2.calculated_fee_multiplier > 10.0);
		assert!(congestion2.calculated_fee_multiplier <= 100.0);
	}

	#[test]
	fn test_price_calculation_precision() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		congestion.add_gas_usage(15_000_000, 2.0);

		// Verify price calculation maintains precision (rounded up)
		let expected_price = (congestion.base_gas_price as f64 * congestion.calculated_fee_multiplier).ceil() as u64;
		assert_eq!(congestion.current_tx_price, expected_price);
	}

	#[test]
	fn test_slot_congestion_debug_formatting() {
		let congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());
		let debug_str = format!("{:?}", congestion);

		assert!(debug_str.contains("SlotCongestion"));
		assert!(debug_str.contains("12345"));
	}

	#[test]
	fn test_slot_congestion_clone() {
		let congestion1 = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());
		let congestion2 = congestion1.clone();

		assert_eq!(congestion1.slot, congestion2.slot);
		assert_eq!(congestion1.base_gas_price, congestion2.base_gas_price);
		assert_eq!(congestion1.total_gas_limit, congestion2.total_gas_limit);
	}

	#[test]
	fn test_slot_congestion_serialization() {
		let congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Test that it can be serialized/deserialized
		let serialized = serde_json::to_string(&congestion).unwrap();
		let deserialized: SlotCongestion = serde_json::from_str(&serialized).unwrap();

		assert_eq!(congestion.slot, deserialized.slot);
		assert_eq!(congestion.base_gas_price, deserialized.base_gas_price);
		assert_eq!(congestion.total_gas_limit, deserialized.total_gas_limit);
	}

	#[test]
	fn test_zero_gas_limit_handling() {
		// Test that zero gas limit is handled gracefully without NaN
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 0, SystemTime::now());

		// Add gas usage with zero limit
		congestion.add_gas_usage(10_000_000, 2.0);

		// Should set gas_used_ratio to 0.0, multiplier to max (100.0), and price to max
		assert_eq!(congestion.gas_used_ratio, 0.0);
		assert_eq!(congestion.calculated_fee_multiplier, 100.0);
		assert_eq!(congestion.current_tx_price, 100_000_000_000); // base * 100

		// All values should be finite
		assert!(congestion.gas_used_ratio.is_finite());
		assert!(congestion.calculated_fee_multiplier.is_finite());
	}

	// Integration tests that would require a real database
	#[tokio::test]
	#[ignore] // Ignore by default since it requires a real database
	async fn test_slot_congestion_crud_operations() {
		// This test would require a real PostgreSQL database
		let pool_result = PgPool::connect_lazy("postgresql://test:test@localhost/test_db");

		if let Ok(pool) = pool_result
			&& pool.acquire().await.is_ok()
		{
			let slot = 12345;
			let base_price = 1_000_000_000;
			let gas_limit = 30_000_000;
			let genesis_time = 1606824023;

			// Test create/get
			let congestion =
				get_or_create_slot_congestion(&pool, slot, base_price, gas_limit, genesis_time).await.unwrap();
			assert_eq!(congestion.slot, slot);
			assert_eq!(congestion.base_gas_price, base_price);

			// Test update
			let updated = update_slot_congestion_gas_usage(&pool, slot, 1_000_000, 2.0).await.unwrap();
			assert_eq!(updated.preconfirmed_gas, 1_000_000);

			// Test get current price
			let price = get_current_gas_price_for_slot(&pool, slot).await.unwrap();
			assert!(price.is_some());
			assert!(price.unwrap() > base_price);
		}
	}

	#[tokio::test]
	#[ignore] // Ignore by default since it requires a real database
	async fn test_slot_congestion_concurrent_updates() {
		// This test would require a real PostgreSQL database
		let pool_result = PgPool::connect_lazy("postgresql://test:test@localhost/test_db");

		if let Ok(pool) = pool_result
			&& pool.acquire().await.is_ok()
		{
			let slot = 12346;
			let base_price = 1_000_000_000;
			let gas_limit = 30_000_000;
			let genesis_time = 1606824023;

			// Create initial congestion
			get_or_create_slot_congestion(&pool, slot, base_price, gas_limit, genesis_time).await.unwrap();

			// Simulate concurrent updates (in real scenario, these would be from different tasks)
			let update1 = update_slot_congestion_gas_usage(&pool, slot, 5_000_000, 2.0).await.unwrap();
			let update2 = update_slot_congestion_gas_usage(&pool, slot, 3_000_000, 2.0).await.unwrap();

			// Second update should have higher gas usage
			assert!(update2.preconfirmed_gas > update1.preconfirmed_gas);
		}
	}

	#[tokio::test]
	#[ignore] // Ignore by default since it requires a real database
	async fn test_slot_congestion_cleanup() {
		// This test would require a real PostgreSQL database
		let pool_result = PgPool::connect_lazy("postgresql://test:test@localhost/test_db");

		if let Ok(pool) = pool_result
			&& pool.acquire().await.is_ok()
		{
			let old_slot = 1000;
			let current_slot = 12347;
			let base_price = 1_000_000_000;
			let gas_limit = 30_000_000;
			let genesis_time = 1606824023;

			// Create old congestion record
			get_or_create_slot_congestion(&pool, old_slot, base_price, gas_limit, genesis_time).await.unwrap();

			// Create current congestion record
			get_or_create_slot_congestion(&pool, current_slot, base_price, gas_limit, genesis_time).await.unwrap();

			// Cleanup old records (keep only 1 hour)
			let _deleted_count = cleanup_old_slot_congestion(&pool, 1).await.unwrap();

			// Verify old record is gone
			let old_price = get_current_gas_price_for_slot(&pool, old_slot).await.unwrap();
			assert!(old_price.is_none());

			// Verify current record still exists
			let current_price = get_current_gas_price_for_slot(&pool, current_slot).await.unwrap();
			assert!(current_price.is_some());
		}
	}

	#[tokio::test]
	#[ignore] // Ignore by default since it requires a real database
	async fn test_congestion_stats_aggregation() {
		// This test would require a real PostgreSQL database
		let pool_result = PgPool::connect_lazy("postgresql://test:test@localhost/test_db");

		if let Ok(pool) = pool_result
			&& pool.acquire().await.is_ok()
		{
			let base_price = 1_000_000_000;
			let gas_limit = 30_000_000;
			let genesis_time = 1606824023;

			// Create multiple congestion records
			for slot in 12348..12353 {
				get_or_create_slot_congestion(&pool, slot, base_price, gas_limit, genesis_time).await.unwrap();
				update_slot_congestion_gas_usage(&pool, slot, slot * 1000, 2.0).await.unwrap();
			}

			// Get aggregated stats
			let stats = get_congestion_stats(&pool).await.unwrap();
			assert!(stats.total_slots_tracked > 0);
			assert!(stats.current_average_congestion >= 0.0);
			assert!(stats.average_fee_multiplier >= 1.0);
		}
	}
}
