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
    /// Create a new slot congestion record
    pub fn new(
        slot: u64,
        base_gas_price: u64,
        total_gas_limit: u64,
        slot_start_time: SystemTime,
    ) -> Self {
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

    /// Update congestion with new gas usage
    pub fn add_gas_usage(&mut self, additional_gas: u64, scaling_factor: f64) {
        self.preconfirmed_gas += additional_gas;
        self.gas_used_ratio = self.preconfirmed_gas as f64 / self.total_gas_limit as f64;

        // Calculate fee multiplier using the congestion formula
        // multiplier = 1 / (1 - (gas_ratio)^k)
        if self.gas_used_ratio >= 1.0 {
            // Prevent division by zero - at 100% usage, use max multiplier
            self.calculated_fee_multiplier = 100.0;
        } else {
            let ratio_powered = self.gas_used_ratio.powf(scaling_factor);
            self.calculated_fee_multiplier = 1.0 / (1.0 - ratio_powered);
        }

        // Apply bounds checking
        self.calculated_fee_multiplier = self.calculated_fee_multiplier.clamp(1.0, 100.0);

        // Calculate final transaction price
        self.current_tx_price = (self.base_gas_price as f64 * self.calculated_fee_multiplier) as u64;
    }
}

/// Get or create slot congestion record for a specific slot
pub async fn get_or_create_slot_congestion(
    pool: &PgPool,
    slot: u64,
    base_gas_price: u64,
    total_gas_limit: u64,
    genesis_time: u64,
) -> Result<SlotCongestion> {
    // First try to get existing record
    if let Some(congestion) = get_slot_congestion(pool, slot).await? {
        return Ok(congestion);
    }

    // Calculate slot start time
    let slot_start_timestamp = genesis_time + (slot * 12); // 12-second slots
    let slot_start_time = UNIX_EPOCH + Duration::from_secs(slot_start_timestamp);

    // Create new record
    let congestion = SlotCongestion::new(slot, base_gas_price, total_gas_limit, slot_start_time);

    let slot_start_time_chrono = sqlx::types::chrono::DateTime::from_timestamp(
        slot_start_timestamp as i64, 0
    ).ok_or_else(|| anyhow::anyhow!("Invalid slot start timestamp"))?.naive_utc();

    let id = sqlx::query!(
        r#"
        INSERT INTO slot_congestion (
            slot, preconfirmed_gas, total_gas_limit, gas_used_ratio,
            base_gas_price, calculated_fee_multiplier, current_tx_price, slot_start_time
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
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
    .fetch_one(pool)
    .await
    .context("Failed to insert slot congestion record")?;

    debug!("Created new slot congestion record for slot {} with ID {}", slot, id.id);

    let mut result = congestion;
    result.id = Some(id.id);
    Ok(result)
}

/// Get slot congestion data for a specific slot
pub async fn get_slot_congestion(
    pool: &PgPool,
    slot: u64,
) -> Result<Option<SlotCongestion>> {
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

/// Update slot congestion with additional gas usage
pub async fn update_slot_congestion_gas_usage(
    pool: &PgPool,
    slot: u64,
    additional_gas: u64,
    scaling_factor: f64,
) -> Result<SlotCongestion> {
    let mut congestion = get_slot_congestion(pool, slot).await?
        .ok_or_else(|| anyhow::anyhow!("Slot congestion record not found for slot {}", slot))?;

    // Update in memory
    congestion.add_gas_usage(additional_gas, scaling_factor);

    // Update in database
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
    .execute(pool)
    .await
    .context("Failed to update slot congestion")?;

    debug!(
        "Updated slot {} congestion: {}% full, {:.2}x multiplier, {} wei price",
        slot,
        (congestion.gas_used_ratio * 100.0),
        congestion.calculated_fee_multiplier,
        congestion.current_tx_price
    );

    Ok(congestion)
}

/// Get current gas price for a slot (the calculated price including congestion)
pub async fn get_current_gas_price_for_slot(
    pool: &PgPool,
    slot: u64,
) -> Result<Option<u64>> {
    let row = sqlx::query!(
        "SELECT current_tx_price FROM slot_congestion WHERE slot = $1",
        slot as i64
    )
    .fetch_optional(pool)
    .await
    .context("Failed to query current gas price for slot")?;

    Ok(row.map(|r| r.current_tx_price as u64))
}

/// Clean up old slot congestion records (older than specified hours)
pub async fn cleanup_old_slot_congestion(
    pool: &PgPool,
    hours_to_keep: u32,
) -> Result<u64> {
    let cutoff_time = SystemTime::now() - Duration::from_secs(hours_to_keep as u64 * 3600);
    let cutoff_timestamp = cutoff_time.duration_since(UNIX_EPOCH)?.as_secs() as i64;

    let cutoff_chrono = sqlx::types::chrono::DateTime::from_timestamp(cutoff_timestamp, 0)
        .ok_or_else(|| anyhow::anyhow!("Invalid cutoff timestamp"))?.naive_utc();

    let result = sqlx::query!(
        "DELETE FROM slot_congestion WHERE slot_start_time < $1",
        cutoff_chrono
    )
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
            "SELECT slot FROM slot_congestion WHERE gas_used_ratio = $1 LIMIT 1",
            stats.max_congestion
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

        // Price should be base_price * multiplier
        let expected_price = (1_000_000_000.0 * congestion.calculated_fee_multiplier) as u64;
        assert_eq!(congestion.current_tx_price, expected_price);
    }

    #[test]
    fn test_high_congestion_bounds() {
        let mut congestion = SlotCongestion::new(
            12345,
            1_000_000_000,
            30_000_000,
            SystemTime::now(),
        );

        // Add 29M gas (96.7% of limit)
        congestion.add_gas_usage(29_000_000, 2.0);

        // Should be bounded to reasonable maximum
        assert!(congestion.calculated_fee_multiplier >= 1.0);
        assert!(congestion.calculated_fee_multiplier <= 100.0);
        assert!(congestion.current_tx_price >= congestion.base_gas_price);
    }

    #[test]
    fn test_full_congestion() {
        let mut congestion = SlotCongestion::new(
            12345,
            1_000_000_000,
            30_000_000,
            SystemTime::now(),
        );

        // Add full gas limit
        congestion.add_gas_usage(30_000_000, 2.0);

        assert_eq!(congestion.gas_used_ratio, 1.0);
        assert_eq!(congestion.calculated_fee_multiplier, 100.0); // Max multiplier
        assert_eq!(congestion.current_tx_price, 100_000_000_000); // 100x base price
    }
}