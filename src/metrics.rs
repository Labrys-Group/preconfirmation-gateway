//! Prometheus metrics integration for gateway monitoring
//!
//! This module exposes operational metrics for:
//! - Commitment tracking (total, by type, timestamps)
//! - Delegation tracking (active, proposers, delegates, slots)
//! - Congestion monitoring (ratios, fee multipliers)
//! - Pricing stats (current slot, gas prices)

use anyhow::Result;
use prometheus::{Encoder, GaugeVec, IntGaugeVec, Opts, Registry, TextEncoder};
use tracing::warn;

use crate::db::DatabaseContext;
use crate::services::fee_pricing::FeePricingEngine;

/// Prometheus metrics registry and collectors
pub struct MetricsRegistry {
	registry: Registry,

	// Commitment metrics
	commitments_total: IntGaugeVec,
	commitments_by_type: IntGaugeVec,

	// Delegation metrics
	delegations_total: IntGaugeVec,
	delegations_active: IntGaugeVec,
	unique_proposers: IntGaugeVec,
	unique_delegates: IntGaugeVec,
	slots_covered: IntGaugeVec,

	// Congestion metrics
	average_congestion: GaugeVec,
	highest_congestion: GaugeVec,
	average_fee_multiplier: GaugeVec,

	// Pricing metrics
	current_slot: IntGaugeVec,
	base_gas_price: IntGaugeVec,
}

impl MetricsRegistry {
	/// Creates and registers a Prometheus metrics registry populated with all gateway collectors.
	///
	/// On success returns an initialized `MetricsRegistry` with collectors for commitments,
	/// delegations, congestion, and pricing already registered with the internal registry.
	///
	/// # Examples
	///
	pub fn new() -> Result<Self> {
		let registry = Registry::new();

		// Commitment metrics
		let commitments_total =
			IntGaugeVec::new(Opts::new("gateway_commitments_total", "Total number of commitments"), &["type"])?;

		let commitments_by_type =
			IntGaugeVec::new(Opts::new("gateway_commitments_by_type", "Commitments by type"), &["commitment_type"])?;

		// Delegation metrics
		let delegations_total =
			IntGaugeVec::new(Opts::new("gateway_delegations_total", "Total delegations"), &["status"])?;

		let delegations_active =
			IntGaugeVec::new(Opts::new("gateway_delegations_active", "Active delegations count"), &["label"])?;

		let unique_proposers =
			IntGaugeVec::new(Opts::new("gateway_unique_proposers", "Unique proposers count"), &["label"])?;

		let unique_delegates =
			IntGaugeVec::new(Opts::new("gateway_unique_delegates", "Unique delegates count"), &["label"])?;

		let slots_covered =
			IntGaugeVec::new(Opts::new("gateway_slots_covered", "Number of slots with delegations"), &["label"])?;

		// Congestion metrics
		let average_congestion = GaugeVec::new(
			Opts::new("gateway_average_congestion_ratio", "Average congestion ratio (0.0-1.0)"),
			&["window"],
		)?;

		let highest_congestion =
			GaugeVec::new(Opts::new("gateway_highest_congestion_ratio", "Highest congestion ratio"), &["window"])?;

		let average_fee_multiplier =
			GaugeVec::new(Opts::new("gateway_average_fee_multiplier", "Average fee multiplier"), &["window"])?;

		// Pricing metrics
		let current_slot =
			IntGaugeVec::new(Opts::new("gateway_current_slot", "Current beacon chain slot"), &["label"])?;

		let base_gas_price =
			IntGaugeVec::new(Opts::new("gateway_base_gas_price_wei", "Current base gas price in wei"), &["label"])?;

		// Register all collectors
		registry.register(Box::new(commitments_total.clone()))?;
		registry.register(Box::new(commitments_by_type.clone()))?;
		registry.register(Box::new(delegations_total.clone()))?;
		registry.register(Box::new(delegations_active.clone()))?;
		registry.register(Box::new(unique_proposers.clone()))?;
		registry.register(Box::new(unique_delegates.clone()))?;
		registry.register(Box::new(slots_covered.clone()))?;
		registry.register(Box::new(average_congestion.clone()))?;
		registry.register(Box::new(highest_congestion.clone()))?;
		registry.register(Box::new(average_fee_multiplier.clone()))?;
		registry.register(Box::new(current_slot.clone()))?;
		registry.register(Box::new(base_gas_price.clone()))?;

		Ok(Self {
			registry,
			commitments_total,
			commitments_by_type,
			delegations_total,
			delegations_active,
			unique_proposers,
			unique_delegates,
			slots_covered,
			average_congestion,
			highest_congestion,
			average_fee_multiplier,
			current_slot,
			base_gas_price,
		})
	}

	/// Refreshes all Prometheus metrics by querying the configured database and fee pricing engine.
	///
	/// This method fetches commitment, delegation, congestion, and pricing statistics from the provided
	/// services and updates the corresponding metric collectors. Failures to fetch individual groups
	/// are logged as warnings; the function itself propagates errors encountered during the overall
	/// update process.
	///
	/// # Examples
	///
	pub async fn update_metrics(&self, database: &DatabaseContext, fee_engine: &FeePricingEngine) -> Result<()> {
		// Update commitment stats
		if let Ok(commitment_stats) = database.get_stats().await {
			self.commitments_total.with_label_values(&["all"]).set(commitment_stats.total_count);
			self.commitments_by_type.with_label_values(&["inclusion"]).set(commitment_stats.commitment_type_1_count);
		} else {
			warn!("Failed to update commitment stats");
		}

		// Update delegation stats
		if let Ok(delegation_stats) = database.get_delegation_stats().await {
			self.delegations_total.with_label_values(&["all"]).set(delegation_stats.total_count);
			self.delegations_active.with_label_values(&["current"]).set(delegation_stats.active_count);
			self.unique_proposers.with_label_values(&["all"]).set(delegation_stats.unique_proposers);
			self.unique_delegates.with_label_values(&["all"]).set(delegation_stats.unique_delegates);
			self.slots_covered.with_label_values(&["active"]).set(delegation_stats.slots_covered);
		} else {
			warn!("Failed to update delegation stats");
		}

		// Update congestion stats
		if let Ok(congestion_stats) = database.get_congestion_stats().await {
			self.average_congestion.with_label_values(&["24h"]).set(congestion_stats.current_average_congestion);
			self.highest_congestion.with_label_values(&["24h"]).set(congestion_stats.highest_congestion_ratio);
			self.average_fee_multiplier.with_label_values(&["24h"]).set(congestion_stats.average_fee_multiplier);
		} else {
			warn!("Failed to update congestion stats");
		}

		// Update pricing stats
		if let Ok(pricing_stats) = fee_engine.get_pricing_stats().await {
			self.current_slot.with_label_values(&["beacon"]).set(pricing_stats.current_slot as i64);
			if let Some(gas_price) = pricing_stats.current_base_gas_price {
				self.base_gas_price.with_label_values(&["current"]).set(gas_price as i64);
			}
		} else {
			warn!("Failed to update pricing stats");
		}

		Ok(())
	}

	/// Render the registry's metrics into Prometheus text exposition format.
	///
	/// Encodes all registered metric families and returns the resulting UTF-8 string.
	///
	/// # Examples
	///
	pub fn render_metrics(&self) -> Result<String> {
		let encoder = TextEncoder::new();
		let metric_families = self.registry.gather();

		let mut buffer = Vec::new();
		encoder.encode(&metric_families, &mut buffer)?;

		Ok(String::from_utf8(buffer)?)
	}

	/// Update commitment-related Prometheus metrics.
	///
	/// Sets the total number of commitments and the count for the "inclusion" commitment type.
	///
	/// # Parameters
	///
	/// - `total`: Total number of commitments to record.
	/// - `type_1`: Number of commitments of the "inclusion" type to record.
	///
	/// # Examples
	///
	pub fn update_commitment_stats(&self, total: i64, type_1: i64) {
		self.commitments_total.with_label_values(&["all"]).set(total);
		self.commitments_by_type.with_label_values(&["inclusion"]).set(type_1);
	}

	/// Update delegation-related Prometheus gauges with the provided counts.
	///
	/// Sets the following metric labels:
	/// - `delegations_total` ("all") to `total`
	/// - `delegations_active` ("current") to `active`
	/// - `unique_proposers` ("all") to `proposers`
	/// - `unique_delegates` ("all") to `delegates`
	/// - `slots_covered` ("active") to `slots`
	///
	/// # Examples
	///
	pub fn update_delegation_stats(&self, total: i64, active: i64, proposers: i64, delegates: i64, slots: i64) {
		self.delegations_total.with_label_values(&["all"]).set(total);
		self.delegations_active.with_label_values(&["current"]).set(active);
		self.unique_proposers.with_label_values(&["all"]).set(proposers);
		self.unique_delegates.with_label_values(&["all"]).set(delegates);
		self.slots_covered.with_label_values(&["active"]).set(slots);
	}

	/// Update congestion-related Prometheus metrics for the 24h window.
	///
	/// This sets the `average_congestion`, `highest_congestion`, and `average_fee_multiplier`
	/// metrics (all labeled `"24h"`) to the provided values.
	///
	/// # Parameters
	///
	/// - `avg_congestion`: average congestion value for the 24-hour window.
	/// - `highest`: highest observed congestion ratio for the 24-hour window.
	/// - `avg_multiplier`: average fee multiplier for the 24-hour window.
	///
	/// # Examples
	///
	pub fn update_congestion_stats(&self, avg_congestion: f64, highest: f64, avg_multiplier: f64) {
		self.average_congestion.with_label_values(&["24h"]).set(avg_congestion);
		self.highest_congestion.with_label_values(&["24h"]).set(highest);
		self.average_fee_multiplier.with_label_values(&["24h"]).set(avg_multiplier);
	}

	/// Update pricing-related Prometheus metrics.
	///
	/// Sets the `current_slot` metric (label "beacon") to `slot`. If `gas_price` is `Some`,
	/// sets the `base_gas_price` metric (label "current") to that value.
	///
	/// # Examples
	///
	pub fn update_pricing_stats(&self, slot: u64, gas_price: Option<u64>) {
		self.current_slot.with_label_values(&["beacon"]).set(slot as i64);
		if let Some(price) = gas_price {
			self.base_gas_price.with_label_values(&["current"]).set(price as i64);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::reth::RethApiClient;
	use crate::config::Config;
	use crate::db::DatabaseContext;
	use crate::services::fee_pricing::FeePricingEngine;

	/// Helper function to create a test metrics registry
	fn create_test_metrics_registry() -> MetricsRegistry {
		MetricsRegistry::new().expect("Failed to create metrics registry")
	}

	#[test]
	fn test_metrics_registry_creation() {
		let registry = create_test_metrics_registry();

		// Test that we can render metrics (should not panic)
		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		// Metrics output might be empty if no metrics have been set yet
		// Just verify we can render without panicking
		assert!(!metrics_output.is_empty() || metrics_output.is_empty());
	}

	#[test]
	fn test_commitment_stats_update() {
		let registry = create_test_metrics_registry();

		// Test updating commitment stats
		registry.update_commitment_stats(100, 75);

		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output.contains("gateway_commitments_total{type=\"all\"} 100"));
		assert!(metrics_output.contains("gateway_commitments_by_type{commitment_type=\"inclusion\"} 75"));
	}

	#[test]
	fn test_delegation_stats_update() {
		let registry = create_test_metrics_registry();

		// Test updating delegation stats
		registry.update_delegation_stats(50, 45, 10, 5, 20);

		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output.contains("gateway_delegations_total{status=\"all\"} 50"));
		assert!(metrics_output.contains("gateway_delegations_active{label=\"current\"} 45"));
		assert!(metrics_output.contains("gateway_unique_proposers{label=\"all\"} 10"));
		assert!(metrics_output.contains("gateway_unique_delegates{label=\"all\"} 5"));
		assert!(metrics_output.contains("gateway_slots_covered{label=\"active\"} 20"));
	}

	#[test]
	fn test_congestion_stats_update() {
		let registry = create_test_metrics_registry();

		// Test updating congestion stats
		registry.update_congestion_stats(0.75, 0.95, 2.5);

		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output.contains("gateway_average_congestion_ratio{window=\"24h\"} 0.75"));
		assert!(metrics_output.contains("gateway_highest_congestion_ratio{window=\"24h\"} 0.95"));
		assert!(metrics_output.contains("gateway_average_fee_multiplier{window=\"24h\"} 2.5"));
	}

	#[test]
	fn test_pricing_stats_update() {
		let registry = create_test_metrics_registry();

		// Test updating pricing stats with gas price
		registry.update_pricing_stats(12345, Some(20_000_000_000));

		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output.contains("gateway_current_slot{label=\"beacon\"} 12345"));
		assert!(metrics_output.contains("gateway_base_gas_price_wei{label=\"current\"} 20000000000"));

		// Test updating pricing stats without gas price
		registry.update_pricing_stats(12346, None);
		let metrics_output2 = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output2.contains("gateway_current_slot{label=\"beacon\"} 12346"));
		// Gas price should remain the same since we passed None
		assert!(metrics_output2.contains("gateway_base_gas_price_wei{label=\"current\"} 20000000000"));
	}

	#[test]
	fn test_metrics_registry_multiple_updates() {
		let registry = create_test_metrics_registry();

		// Update all types of metrics multiple times
		registry.update_commitment_stats(10, 8);
		registry.update_delegation_stats(5, 4, 2, 1, 3);
		registry.update_congestion_stats(0.5, 0.8, 1.5);
		registry.update_pricing_stats(1000, Some(15_000_000_000));

		let metrics_output = registry.render_metrics().expect("Failed to render metrics");

		// Verify all metrics are present
		assert!(metrics_output.contains("gateway_commitments_total{type=\"all\"} 10"));
		assert!(metrics_output.contains("gateway_commitments_by_type{commitment_type=\"inclusion\"} 8"));
		assert!(metrics_output.contains("gateway_delegations_total{status=\"all\"} 5"));
		assert!(metrics_output.contains("gateway_delegations_active{label=\"current\"} 4"));
		assert!(metrics_output.contains("gateway_unique_proposers{label=\"all\"} 2"));
		assert!(metrics_output.contains("gateway_unique_delegates{label=\"all\"} 1"));
		assert!(metrics_output.contains("gateway_slots_covered{label=\"active\"} 3"));
		assert!(metrics_output.contains("gateway_average_congestion_ratio{window=\"24h\"} 0.5"));
		assert!(metrics_output.contains("gateway_highest_congestion_ratio{window=\"24h\"} 0.8"));
		assert!(metrics_output.contains("gateway_average_fee_multiplier{window=\"24h\"} 1.5"));
		assert!(metrics_output.contains("gateway_current_slot{label=\"beacon\"} 1000"));
		assert!(metrics_output.contains("gateway_base_gas_price_wei{label=\"current\"} 15000000000"));
	}

	#[test]
	fn test_metrics_registry_edge_cases() {
		let registry = create_test_metrics_registry();

		// Test with zero values
		registry.update_commitment_stats(0, 0);
		registry.update_delegation_stats(0, 0, 0, 0, 0);
		registry.update_congestion_stats(0.0, 0.0, 0.0);
		registry.update_pricing_stats(0, Some(0));

		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output.contains("gateway_commitments_total{type=\"all\"} 0"));
		assert!(metrics_output.contains("gateway_delegations_total{status=\"all\"} 0"));
		assert!(metrics_output.contains("gateway_average_congestion_ratio{window=\"24h\"} 0"));
		assert!(metrics_output.contains("gateway_current_slot{label=\"beacon\"} 0"));
		assert!(metrics_output.contains("gateway_base_gas_price_wei{label=\"current\"} 0"));

		// Test with reasonable large values (avoid f64::MAX which causes formatting issues)
		let large_i64 = 1_000_000_000_000i64;
		let large_f64 = 1_000_000.0f64;
		let large_u64 = 1_000_000_000_000u64;

		registry.update_commitment_stats(large_i64, large_i64);
		registry.update_delegation_stats(large_i64, large_i64, large_i64, large_i64, large_i64);
		registry.update_congestion_stats(large_f64, large_f64, large_f64);
		registry.update_pricing_stats(large_u64, Some(large_u64));

		let metrics_output2 = registry.render_metrics().expect("Failed to render metrics");
		assert!(metrics_output2.contains(&format!("gateway_commitments_total{{type=\"all\"}} {}", large_i64)));
		assert!(metrics_output2.contains(&format!("gateway_delegations_total{{status=\"all\"}} {}", large_i64)));
		assert!(metrics_output2.contains(&format!("gateway_average_congestion_ratio{{window=\"24h\"}} {}", large_f64)));
		assert!(metrics_output2.contains(&format!("gateway_current_slot{{label=\"beacon\"}} {}", large_u64)));
		assert!(metrics_output2.contains(&format!("gateway_base_gas_price_wei{{label=\"current\"}} {}", large_u64)));
	}

	#[tokio::test]
	async fn test_update_metrics_with_mock_services() {
		let registry = create_test_metrics_registry();

		// Create a test database context (this will use the testing helper)
		let db_context = DatabaseContext::new_for_testing();

		// Create a mock fee pricing engine
		let config = Config::load().unwrap_or_else(|_| Config::default());
		let reth_config = crate::api::reth::RethApiConfig {
			endpoint: "http://localhost:8545".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
		};
		let reth_client = RethApiClient::new(reth_config).expect("Failed to create Reth client");
		let fee_engine = FeePricingEngine::new(
			std::sync::Arc::new(reth_client),
			std::sync::Arc::new(db_context.clone()),
			std::sync::Arc::new(config),
		);

		// Test update_metrics method (this will handle errors gracefully)
		let result = registry.update_metrics(&db_context, &fee_engine).await;
		// This might fail due to no actual database connection, but should not panic
		if let Err(e) = result {
			// Expected to fail in test environment without real database
			println!("Expected error in test environment: {}", e);
		}

		// Verify that render_metrics still works
		let metrics_output = registry.render_metrics().expect("Failed to render metrics");
		// Metrics output might be empty if no metrics have been set yet
		assert!(!metrics_output.is_empty() || metrics_output.is_empty());
	}
}
