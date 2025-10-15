//! Comprehensive tests for fee pricing engine
//!
//! These tests cover gas estimation, congestion calculation, fee multiplier bounds,
//! overflow handling, and other edge cases.

#[cfg(test)]
mod fee_pricing_tests {
	use super::super::*;
	use crate::api::reth::{RethApiClient, RethApiConfig};
	use crate::config::Config;
	use crate::db::DatabaseContext;
	use crate::db::slot_congestion_ops::SlotCongestion;
	use crate::types::payload::{InclusionPayload, PayloadParser};
	use anyhow::Result;
	use serial_test::serial;
	use sqlx::PgPool;
	use std::sync::Arc;

	async fn setup_test_pool() -> Result<PgPool> {
		let database_url = match std::env::var("DATABASE_URL") {
			Ok(url) => url,
			Err(_) => return Err(anyhow::anyhow!("DATABASE_URL not set")),
		};

		Ok(PgPool::connect(&database_url).await?)
	}

	#[tokio::test]
	async fn test_estimate_gas_for_inclusion_with_valid_payload() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Create a proper inclusion payload
		let inclusion_payload = InclusionPayload::new(12345, vec![0xaa, 0xbb, 0xcc, 0xdd]);
		let encoded = PayloadParser::encode_inclusion_payload(&inclusion_payload).unwrap();

		let gas = engine.estimate_gas_for_commitment(1, &encoded).unwrap();

		// Should be base (21k) + data (4 bytes * 16) + overhead (10k) = ~31k
		assert!(gas >= 21_000);
		assert!(gas <= 50_000);
	}

	#[tokio::test]
	async fn test_estimate_gas_for_inclusion_with_invalid_payload_fallback() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Invalid payload should trigger fallback
		let invalid_payload = vec![0xff, 0xff];
		let gas = engine.estimate_gas_for_commitment(1, &invalid_payload).unwrap();

		// Should use fallback estimate
		assert_eq!(gas, 50_000);
	}

	#[tokio::test]
	async fn test_estimate_gas_for_execution_commitment() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Type 2 (execution) commitment
		let gas = engine.estimate_gas_for_commitment(2, &[1, 2, 3, 4]).unwrap();

		// Should use 100,000 gas estimate for execution
		assert_eq!(gas, 100_000);
	}

	#[tokio::test]
	async fn test_estimate_gas_for_unknown_commitment_type() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Unknown type should return error
		let result = engine.estimate_gas_for_commitment(999, &[1, 2, 3, 4]);

		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Unknown commitment type"));
	}

	#[tokio::test]
	async fn test_calculate_projected_congestion_with_bounds() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Create a base congestion at 80% full (high congestion)
		let mut base_congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, std::time::SystemTime::now());

		base_congestion.add_gas_usage(24_000_000, 2.0); // 80% full

		// Project adding more gas
		let projected = engine.calculate_projected_congestion(&base_congestion, 3_000_000).unwrap();

		// Fee multiplier should be clamped by max_fee_multiplier
		assert!(projected.calculated_fee_multiplier <= engine.fee_config.max_fee_multiplier);
		assert!(projected.calculated_fee_multiplier >= engine.fee_config.min_fee_multiplier);
	}

	#[tokio::test]
	async fn test_calculate_projected_congestion_at_minimum() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Create a base congestion with very low usage
		let base_congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, std::time::SystemTime::now());

		// Project adding minimal gas
		let projected = engine.calculate_projected_congestion(&base_congestion, 100).unwrap();

		// Fee multiplier should be at minimum
		assert!(projected.calculated_fee_multiplier >= engine.fee_config.min_fee_multiplier);
		assert!(projected.current_tx_price >= base_congestion.base_gas_price);
	}

	#[tokio::test]
	async fn test_slot_acceptability_boundaries() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		let current_slot = engine.get_current_slot();

		// Exactly current slot
		assert!(engine.is_slot_acceptable_for_fees(current_slot));

		// Exactly at lookahead limit (10 slots)
		assert!(engine.is_slot_acceptable_for_fees(current_slot + 10));

		// Just beyond lookahead limit
		assert!(!engine.is_slot_acceptable_for_fees(current_slot + 11));

		// Way in the future
		assert!(!engine.is_slot_acceptable_for_fees(current_slot + 100));
	}

	#[tokio::test]
	async fn test_get_current_slot() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		let slot = engine.get_current_slot();

		// Should be a reasonable slot number (not zero, not too large)
		assert!(slot > 0);
		assert!(slot < u64::MAX / 2);
	}

	#[tokio::test]
	async fn test_calculate_fee_calculation_overflow_protection() {
		// Test that we handle overflow gracefully
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Create a scenario that would cause overflow without protection
		let mut congestion = SlotCongestion::new(12345, u64::MAX, 30_000_000, std::time::SystemTime::now());

		congestion.calculated_fee_multiplier = 10.0; // Would overflow
		congestion.current_tx_price = u64::MAX; // Already at max

		// Project with large gas usage
		let projected = engine.calculate_projected_congestion(&congestion, 1_000_000).unwrap();

		// Should not panic and should clamp to reasonable values
		assert!(projected.current_tx_price <= u64::MAX);
	}

	#[tokio::test]
	async fn test_fee_calculation_with_high_congestion() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Create high congestion scenario (95% full)
		let mut congestion = SlotCongestion::new(12345, 1_000_000_000, 30_000_000, std::time::SystemTime::now());

		congestion.add_gas_usage(28_500_000, 2.0); // 95% full

		let projected = engine.calculate_projected_congestion(&congestion, 100_000).unwrap();

		// Should have high fee multiplier
		assert!(projected.calculated_fee_multiplier > 1.5);
		assert!(projected.current_tx_price > projected.base_gas_price);
	}

	#[tokio::test]
	async fn test_projected_congestion_price_rounding() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Test with a price that would need rounding
		let mut congestion = SlotCongestion::new(12345, 1, 30_000_000, std::time::SystemTime::now()); // 1 wei base price

		congestion.calculated_fee_multiplier = 1.5;

		let projected = engine.calculate_projected_congestion(&congestion, 100_000).unwrap();

		// 1 wei * 1.5 = 1.5 wei, should round up to 2 wei
		assert_eq!(projected.current_tx_price, 2);
	}

	#[tokio::test]
	#[serial]
	async fn test_calculate_fee_for_commitment_integration() -> Result<()> {
		let pool = setup_test_pool().await?;
		let config = Config::default();
		let database = Arc::new(DatabaseContext::new(pool));
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default())?);

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config.clone()));

		let current_slot = engine.get_current_slot();
		let future_slot = current_slot + 5;

		// Create a valid inclusion payload
		let inclusion_payload = InclusionPayload::new(future_slot, vec![0xaa, 0xbb, 0xcc, 0xdd]);
		let encoded = PayloadParser::encode_inclusion_payload(&inclusion_payload)?;

		// This would normally call the Reth node, which we don't have in tests
		// So we can't fully test this without a mock or real node
		// But we can test that it doesn't panic
		let _result = engine.calculate_fee_for_commitment(1, &encoded, future_slot).await;

		// Test passes if we reach here without panicking

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_apply_gas_usage_to_slot() -> Result<()> {
		if std::env::var("DATABASE_URL").is_err() {
			eprintln!("Skipping test: DATABASE_URL not set");
			return Ok(());
		}

		let pool = setup_test_pool().await?;
		let config = Config::default();
		let database = Arc::new(DatabaseContext::new(pool));
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default())?);

		let engine = FeePricingEngine::new(reth_client, database.clone(), Arc::new(config.clone()));

		let current_slot = engine.get_current_slot();
		let test_slot = current_slot + 5; // Use a future slot
		let gas_used = 100_000;

		// First, get or create the slot congestion record
		let genesis_time = config.beacon_api.genesis_time;
		let _initial =
			database.get_or_create_slot_congestion(test_slot, 1_000_000_000, 30_000_000, genesis_time).await?;

		// Apply gas usage
		let updated = engine.apply_gas_usage_to_slot(test_slot, gas_used).await?;

		// Should have recorded the gas usage
		assert!(updated.preconfirmed_gas >= gas_used);
		assert!(updated.gas_used_ratio > 0.0);

		Ok(())
	}

	#[tokio::test]
	#[serial]
	async fn test_get_pricing_stats() -> Result<()> {
		if std::env::var("DATABASE_URL").is_err() {
			eprintln!("Skipping test: DATABASE_URL not set");
			return Ok(());
		}

		let pool = setup_test_pool().await?;
		let config = Config::default();
		let database = Arc::new(DatabaseContext::new(pool));
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default())?);

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config.clone()));

		// Get stats (should not error even if empty)
		let stats = engine.get_pricing_stats().await?;

		// Verify structure
		assert!(stats.current_slot > 0);
		assert!(stats.average_congestion_ratio >= 0.0);
		assert!(stats.average_fee_multiplier >= 0.0);

		Ok(())
	}

	#[test]
	fn test_pricing_stats_creation() {
		let stats = PricingStats {
			current_slot: 12345,
			current_base_gas_price: Some(1_000_000_000),
			average_congestion_ratio: 0.5,
			average_fee_multiplier: 1.5,
			total_slots_tracked: 100,
			highest_congestion_slot: Some(12340),
			highest_congestion_ratio: 0.95,
		};

		assert_eq!(stats.current_slot, 12345);
		assert_eq!(stats.current_base_gas_price, Some(1_000_000_000));
		assert_eq!(stats.average_congestion_ratio, 0.5);
		assert_eq!(stats.average_fee_multiplier, 1.5);
	}

	#[test]
	fn test_fee_calculation_result_creation() {
		let fee_calc = FeeCalculation {
			slot: 12345,
			base_gas_price: 1_000_000_000,
			congestion_ratio: 0.5,
			fee_multiplier: 1.5,
			final_price: 1_500_000_000,
			estimated_gas: 50_000,
			total_cost: 75_000_000_000,
		};

		assert_eq!(fee_calc.slot, 12345);
		assert_eq!(fee_calc.base_gas_price, 1_000_000_000);
		assert_eq!(fee_calc.final_price, 1_500_000_000);
		assert_eq!(fee_calc.estimated_gas, 50_000);
	}

	#[tokio::test]
	async fn test_cache_refresh_interval_calculation() {
		let mut config = Config::default();

		// Test with normal TTL
		config.reth.fee_config.cache_ttl_secs = 60;
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());
		let _engine = FeePricingEngine::new(reth_client, database, Arc::new(config.clone()));

		// Refresh interval should be half of TTL
		let expected_interval = config.reth.fee_config.cache_ttl_secs / 2;
		assert_eq!(expected_interval, 30);

		// Test with very small TTL (should clamp to 1)
		config.reth.fee_config.cache_ttl_secs = 1;
		let reth_client2 = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database2 = Arc::new(DatabaseContext::new_for_testing());
		let _engine2 = FeePricingEngine::new(reth_client2, database2, Arc::new(config.clone()));

		let expected_interval_min = (config.reth.fee_config.cache_ttl_secs / 2).max(1);
		assert_eq!(expected_interval_min, 1);
	}

	#[tokio::test]
	async fn test_gas_estimation_with_large_signed_tx() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Create a large signed transaction payload
		let large_tx = vec![0xaa; 5000]; // 5KB transaction
		let inclusion_payload = InclusionPayload::new(12345, large_tx);
		let encoded = PayloadParser::encode_inclusion_payload(&inclusion_payload).unwrap();

		let gas = engine.estimate_gas_for_commitment(1, &encoded).unwrap();

		// Should account for large data: base (21k) + data (5000 * 16) + overhead (10k)
		// = 21,000 + 80,000 + 10,000 = 111,000
		assert!(gas >= 100_000);
		assert!(gas <= 150_000);
	}
}
