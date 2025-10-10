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
