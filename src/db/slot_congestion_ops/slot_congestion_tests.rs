//! Comprehensive integration tests for slot congestion database operations

#[cfg(test)]
mod slot_congestion_db_tests {
	use super::super::*;
	use anyhow::Result;
	use serial_test::serial;
	use sqlx::PgPool;
	use std::time::SystemTime;

	async fn setup_test_pool() -> Result<PgPool> {
		let database_url = std::env::var("DATABASE_URL")
			.unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/preconfirmation_gateway".to_string());

		Ok(PgPool::connect(&database_url).await?)
	}

	#[tokio::test]
	#[serial]
	async fn test_get_or_create_slot_congestion() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let slot = 888777666; // Unique slot
		let base_gas_price = 1_000_000_000;
		let gas_limit = 30_000_000;
		let genesis_time = 1606824023;

		// First call should create the record
		let congestion1 = get_or_create_slot_congestion(&pool, slot, base_gas_price, gas_limit, genesis_time).await?;

		assert_eq!(congestion1.slot, slot);
		assert_eq!(congestion1.base_gas_price, base_gas_price);
		assert_eq!(congestion1.total_gas_limit, gas_limit);

		// Second call should return the same record
		let congestion2 = get_or_create_slot_congestion(&pool, slot, base_gas_price, gas_limit, genesis_time).await?;

		assert_eq!(congestion2.slot, slot);
		// ID should be the same (same record)
		assert_eq!(congestion1.id, congestion2.id);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_update_slot_congestion_gas_usage() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let slot = 777666555; // Unique slot
		let base_gas_price = 1_000_000_000;
		let gas_limit = 30_000_000;
		let genesis_time = 1606824023;

		// Create initial record
		let _initial = get_or_create_slot_congestion(&pool, slot, base_gas_price, gas_limit, genesis_time).await?;

		// Update with gas usage
		let gas_used = 5_000_000;
		let scaling_factor = 2.0;
		let updated = update_slot_congestion_gas_usage(&pool, slot, gas_used, scaling_factor).await?;

		assert_eq!(updated.preconfirmed_gas, gas_used);
		assert_eq!(updated.gas_used_ratio, gas_used as f64 / gas_limit as f64);
		assert!(updated.calculated_fee_multiplier >= 1.0);

		// Update again (should accumulate)
		let updated2 = update_slot_congestion_gas_usage(&pool, slot, gas_used, scaling_factor).await?;

		assert_eq!(updated2.preconfirmed_gas, gas_used * 2);
		assert_eq!(updated2.gas_used_ratio, (gas_used * 2) as f64 / gas_limit as f64);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_get_slot_congestion_existing() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let slot = 666555444; // Unique slot
		let base_gas_price = 1_000_000_000;
		let gas_limit = 30_000_000;
		let genesis_time = 1606824023;

		// Create a record
		let created = get_or_create_slot_congestion(&pool, slot, base_gas_price, gas_limit, genesis_time).await?;

		// Retrieve it
		let retrieved = get_slot_congestion(&pool, slot).await?;

		assert!(retrieved.is_some());
		let retrieved_congestion = retrieved.unwrap();
		assert_eq!(retrieved_congestion.slot, slot);
		assert_eq!(retrieved_congestion.base_gas_price, base_gas_price);
		assert_eq!(retrieved_congestion.id, created.id);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_get_slot_congestion_not_found() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let non_existent_slot = 999999999999; // Very unlikely to exist

		// Should return None
		let result = get_slot_congestion(&pool, non_existent_slot).await?;

		assert!(result.is_none());

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_get_congestion_stats() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;

		// Get stats (should not error even if empty)
		let stats = get_congestion_stats(&pool).await?;

		// Verify structure (all values should be valid)
		assert!(stats.current_average_congestion >= 0.0);
		assert!(stats.average_fee_multiplier >= 0.0);
		assert!(stats.highest_congestion_ratio >= 0.0);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_get_congestion_stats_with_data() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let genesis_time = 1606824023;

		// Get baseline stats
		let stats_before = get_congestion_stats(&pool).await?;

		// Create several congestion records
		for i in 0..5 {
			let slot = 555444333 + i;
			let _congestion =
				get_or_create_slot_congestion(&pool, slot, 1_000_000_000, 30_000_000, genesis_time).await?;

			// Add some gas usage
			update_slot_congestion_gas_usage(&pool, slot, 10_000_000, 2.0).await?;
		}

		// Get updated stats
		let stats_after = get_congestion_stats(&pool).await?;

		// Should have more slots tracked
		assert!(stats_after.total_slots_tracked > stats_before.total_slots_tracked);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_concurrent_gas_usage_updates() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let slot = 444333222; // Unique slot
		let base_gas_price = 1_000_000_000;
		let gas_limit = 30_000_000;
		let genesis_time = 1606824023;

		// Create initial record
		let _initial = get_or_create_slot_congestion(&pool, slot, base_gas_price, gas_limit, genesis_time).await?;

		// Spawn multiple concurrent updates
		let mut handles = vec![];

		for _i in 0..5 {
			let pool_clone = pool.clone();
			let handle =
				tokio::spawn(async move { update_slot_congestion_gas_usage(&pool_clone, slot, 1_000_000, 2.0).await });

			handles.push(handle);
		}

		// Wait for all to complete
		for handle in handles {
			let result = handle.await.unwrap();
			assert!(result.is_ok());
		}

		// Final gas should be 5M (5 updates * 1M each)
		let final_congestion = get_slot_congestion(&pool, slot).await?.unwrap();
		assert_eq!(final_congestion.preconfirmed_gas, 5_000_000);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_update_nonexistent_slot_error() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let non_existent_slot = 111222333444; // Very unlikely to exist

		// Try to update non-existent slot (should error)
		let result = update_slot_congestion_gas_usage(&pool, non_existent_slot, 1_000_000, 2.0).await;

		assert!(result.is_err());

		Ok(())
	}

	#[test]
	fn test_slot_congestion_edge_cases() {
		// Test with zero gas limit (edge case)
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 0, SystemTime::now());

		congestion.add_gas_usage(100, 2.0);

		// Should handle division by zero gracefully
		assert_eq!(congestion.gas_used_ratio, 0.0);
		assert_eq!(congestion.calculated_fee_multiplier, 100.0); // Max multiplier

		// Test with maximum values
		let mut congestion_max = SlotCongestion::new(12345, u64::MAX, u64::MAX, SystemTime::now());

		congestion_max.add_gas_usage(100, 2.0);

		// Should not panic or overflow
		assert!(congestion_max.gas_used_ratio >= 0.0);
		assert!(congestion_max.calculated_fee_multiplier >= 1.0);
	}

	#[test]
	fn test_slot_congestion_saturating_add() {
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, SystemTime::now());

		// Add large amount that could overflow
		congestion.add_gas_usage(u64::MAX / 2, 2.0);
		congestion.add_gas_usage(u64::MAX / 2, 2.0);

		// Should saturate at MAX, not panic
		assert!(congestion.preconfirmed_gas > 0);
	}

	#[test]
	fn test_different_base_prices() {
		let mut congestion_low = SlotCongestion::new(12345, 1, 30_000_000, SystemTime::now());
		let mut congestion_high = SlotCongestion::new(12346, 1_000_000_000_000, 30_000_000, SystemTime::now());

		// Add same gas to both
		congestion_low.add_gas_usage(15_000_000, 2.0);
		congestion_high.add_gas_usage(15_000_000, 2.0);

		// Should have same multiplier but different prices
		assert_eq!(congestion_low.calculated_fee_multiplier, congestion_high.calculated_fee_multiplier);
		assert!(congestion_high.current_tx_price > congestion_low.current_tx_price);
	}

	#[test]
	fn test_congestion_stats_debug() {
		let stats = CongestionStats {
			total_slots_tracked: 100,
			current_average_congestion: 0.5,
			highest_congestion_slot: Some(12345),
			highest_congestion_ratio: 0.95,
			average_fee_multiplier: 2.5,
		};

		let debug_str = format!("{:?}", stats);
		assert!(debug_str.contains("CongestionStats"));
		assert!(debug_str.contains("100"));
	}

	#[tokio::test]
	#[serial]
	async fn test_slot_congestion_with_realistic_gas_prices() -> Result<()> {
		let _ = std::env::var("DATABASE_URL").unwrap();

		let pool = setup_test_pool().await?;
		let slot = 333222111; // Unique slot

		// Realistic gas prices (1 gwei to 1000 gwei)
		let low_price = 1_000_000_000; // 1 gwei
		let medium_price = 50_000_000_000; // 50 gwei
		let high_price = 1_000_000_000_000; // 1000 gwei

		let gas_limit = 30_000_000;
		let genesis_time = 1606824023;

		// Test with low price
		let congestion_low = get_or_create_slot_congestion(&pool, slot, low_price, gas_limit, genesis_time).await?;
		assert_eq!(congestion_low.base_gas_price, low_price);

		// Test with medium price
		let congestion_med =
			get_or_create_slot_congestion(&pool, slot + 1, medium_price, gas_limit, genesis_time).await?;
		assert_eq!(congestion_med.base_gas_price, medium_price);

		// Test with high price
		let congestion_high =
			get_or_create_slot_congestion(&pool, slot + 2, high_price, gas_limit, genesis_time).await?;
		assert_eq!(congestion_high.base_gas_price, high_price);

		Ok(())
	}
}
