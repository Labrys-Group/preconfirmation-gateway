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
	/// Create a new metrics registry with all collectors
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

	/// Update all metrics from database and services
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

	/// Render metrics in Prometheus text format
	pub fn render_metrics(&self) -> Result<String> {
		let encoder = TextEncoder::new();
		let metric_families = self.registry.gather();

		let mut buffer = Vec::new();
		encoder.encode(&metric_families, &mut buffer)?;

		Ok(String::from_utf8(buffer)?)
	}

	/// Update commitment metrics from stats
	pub fn update_commitment_stats(&self, total: i64, type_1: i64) {
		self.commitments_total.with_label_values(&["all"]).set(total);
		self.commitments_by_type.with_label_values(&["inclusion"]).set(type_1);
	}

	/// Update delegation metrics from stats
	pub fn update_delegation_stats(&self, total: i64, active: i64, proposers: i64, delegates: i64, slots: i64) {
		self.delegations_total.with_label_values(&["all"]).set(total);
		self.delegations_active.with_label_values(&["current"]).set(active);
		self.unique_proposers.with_label_values(&["all"]).set(proposers);
		self.unique_delegates.with_label_values(&["all"]).set(delegates);
		self.slots_covered.with_label_values(&["active"]).set(slots);
	}

	/// Update congestion metrics from stats
	pub fn update_congestion_stats(&self, avg_congestion: f64, highest: f64, avg_multiplier: f64) {
		self.average_congestion.with_label_values(&["24h"]).set(avg_congestion);
		self.highest_congestion.with_label_values(&["24h"]).set(highest);
		self.average_fee_multiplier.with_label_values(&["24h"]).set(avg_multiplier);
	}

	/// Update pricing metrics
	pub fn update_pricing_stats(&self, slot: u64, gas_price: Option<u64>) {
		self.current_slot.with_label_values(&["beacon"]).set(slot as i64);
		if let Some(price) = gas_price {
			self.base_gas_price.with_label_values(&["current"]).set(price as i64);
		}
	}
}
