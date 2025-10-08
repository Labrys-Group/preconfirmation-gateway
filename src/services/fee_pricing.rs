use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::api::reth::{GasPriceInfo, RethApiClient};
use crate::config::{Config, FeeConfig};
use crate::db::DatabaseContext;
use crate::db::slot_congestion_ops::SlotCongestion;
use crate::types::beacon::BeaconTiming;

/// Dynamic fee pricing engine that implements congestion-based pricing
#[derive(Clone)]
pub struct FeePricingEngine {
	reth_client: Arc<RethApiClient>,
	database: Arc<DatabaseContext>,
	config: Arc<Config>,
	fee_config: FeeConfig,
}

/// Fee calculation result
#[derive(Debug, Clone)]
pub struct FeeCalculation {
	/// Slot this fee applies to
	pub slot: u64,
	/// Base gas price from Reth oracle (wei)
	pub base_gas_price: u64,
	/// Current congestion ratio (0.0 to 1.0)
	pub congestion_ratio: f64,
	/// Fee multiplier from congestion formula
	pub fee_multiplier: f64,
	/// Final transaction price (wei)
	pub final_price: u64,
	/// Estimated gas for this transaction
	pub estimated_gas: u64,
	/// Total cost for this transaction (wei)
	pub total_cost: u64,
}

impl FeePricingEngine {
	/// Constructs a new FeePricingEngine using the provided clients and configuration.
	///
	/// Clones the fee-related configuration from `config` and stores the provided
	/// `reth_client`, `database`, and `config` as shared references.
	///
	/// # Examples
	///
	/// ```no_run
	/// use std::sync::Arc;
	/// let reth_client = Arc::new(/* RethApiClient */ unimplemented!());
	/// let database = Arc::new(/* DatabaseContext */ unimplemented!());
	/// let config = Arc::new(/* Config */ unimplemented!());
	/// let engine = FeePricingEngine::new(reth_client, database, config);
	/// ```
	pub fn new(reth_client: Arc<RethApiClient>, database: Arc<DatabaseContext>, config: Arc<Config>) -> Self {
		Self { reth_client, database, fee_config: config.reth.fee_config.clone(), config }
	}

	/// Compute the fee for submitting a commitment at a given slot.
	///
	/// This performs a full pricing calculation:
	/// 1. obtains the current base gas price from the Reth oracle,
	/// 2. estimates the gas required for the provided commitment type and payload,
	/// 3. loads or creates congestion tracking for the target slot,
	/// 4. projects the congestion and resulting per-gas price after adding the estimated gas,
	/// 5. computes the total cost (per-gas price * estimated gas) with overflow checking.
	///
	/// Errors are returned if fetching the gas price, estimating gas, accessing the database,
	/// or calculating the total cost (overflow) fails.
	///
	/// # Examples
	///
	/// ```
	/// # use std::sync::Arc;
	/// # async fn example(engine: Arc<crate::services::FeePricingEngine>) -> anyhow::Result<()> {
	/// let commitment_type = 1_u64;
	/// let payload: &[u8] = b"...";
	/// let slot = engine.get_current_slot();
	/// let fee = engine.calculate_fee_for_commitment(commitment_type, payload, slot).await?;
	/// assert!(fee.estimated_gas > 0);
	/// # Ok(())
	/// # }
	/// ```
	pub async fn calculate_fee_for_commitment(
		&self,
		commitment_type: u64,
		payload: &[u8],
		slot: u64,
	) -> Result<FeeCalculation> {
		debug!("Calculating fee for commitment type {} in slot {}", commitment_type, slot);

		// 1. Get current base gas price from Reth
		let gas_price_info = self.get_cached_gas_price().await?;

		// 2. Estimate gas usage for this commitment
		let estimated_gas = self.estimate_gas_for_commitment(commitment_type, payload)?;

		// 3. Get or create slot congestion tracking
		let genesis_time = self.config.beacon_api.genesis_time;
		let gas_price_u64 = gas_price_info.gas_price_as_u64_clamped();
		let congestion = self
			.database
			.get_or_create_slot_congestion(slot, gas_price_u64, self.fee_config.default_gas_limit, genesis_time)
			.await?;

		// 4. Calculate what the price would be if we add this gas usage
		let projected_congestion = self.calculate_projected_congestion(&congestion, estimated_gas)?;

		// Calculate total cost with overflow checking
		// Use checked_mul to detect overflow instead of silently wrapping
		let total_cost = projected_congestion.current_tx_price.checked_mul(estimated_gas).context(format!(
			"Total cost calculation overflow: {} wei/gas * {} gas would exceed u64::MAX",
			projected_congestion.current_tx_price, estimated_gas
		))?;

		let fee_calculation = FeeCalculation {
			slot,
			base_gas_price: gas_price_u64,
			congestion_ratio: projected_congestion.gas_used_ratio,
			fee_multiplier: projected_congestion.calculated_fee_multiplier,
			final_price: projected_congestion.current_tx_price,
			estimated_gas,
			total_cost,
		};

		info!(
			"Fee calculated for slot {}: {:.2}% congestion, {:.2}x multiplier, {} wei/gas, {} total cost",
			slot,
			projected_congestion.gas_used_ratio * 100.0,
			projected_congestion.calculated_fee_multiplier,
			projected_congestion.current_tx_price,
			fee_calculation.total_cost
		);

		Ok(fee_calculation)
	}

	/// Apply observed gas usage to a slot's congestion data.
	///
	/// This updates the persisted SlotCongestion for `slot` by adding `gas_used` (subject to the engine's scaling)
	/// and returns the updated congestion record.
	///
	/// # Parameters
	///
	/// - `slot`: Target slot to apply the observed gas usage to.
	/// - `gas_used`: Observed gas units consumed by the commitment for `slot`.
	///
	/// # Returns
	///
	/// The updated `SlotCongestion` after applying the observed gas usage.
	///
	/// # Examples
	///
	/// ```
	/// # async fn example(engine: &crate::services::fee_pricing::FeePricingEngine) {
	/// let updated = engine.apply_gas_usage_to_slot(42, 12_345).await.unwrap();
	/// // updated now reflects the applied gas usage for slot 42
	/// assert!(updated.gas_used_ratio >= 0.0 && updated.gas_used_ratio <= 1.0);
	/// # }
	/// ```
	pub async fn apply_gas_usage_to_slot(&self, slot: u64, gas_used: u64) -> Result<SlotCongestion> {
		debug!("Applying {} gas usage to slot {}", gas_used, slot);

		let updated_congestion =
			self.database.update_slot_congestion_gas_usage(slot, gas_used, self.fee_config.scaling_factor).await?;

		info!(
			"Updated slot {} congestion: {:.2}% full, {:.2}x multiplier",
			slot,
			updated_congestion.gas_used_ratio * 100.0,
			updated_congestion.calculated_fee_multiplier
		);

		Ok(updated_congestion)
	}

	/// Retrieves the current gas price from the configured Reth node.
	///
	/// Currently this always requests fresh data from the Reth client; caching based on `fee_config.cache_ttl_secs` is planned but not yet implemented.
	///
	/// # Errors
	///
	/// Returns an error if the Reth node query fails.
	///
	/// # Examples
	///
	/// ```
	/// // `engine` is a `FeePricingEngine` instance.
	/// let info = engine.get_cached_gas_price().await.unwrap();
	/// assert!(info.base_gas_price > 0);
	/// ```
	async fn get_cached_gas_price(&self) -> Result<GasPriceInfo> {
		// TODO: Implement caching based on fee_config.cache_ttl_secs
		// For now, fetch fresh data each time
		self.reth_client.get_gas_price().await.context("Failed to get gas price from Reth node")
	}

	/// Estimate gas usage for a commitment based on its type and payload.
	///
	/// This returns a conservative gas estimate used for congestion and fee projections:
	/// - Commitment type `1` (inclusion): attempts to parse an inclusion payload and estimates gas as
	///   21,000 (base) + 16 gas per byte of the signed transaction + a 10,000 overhead. If parsing
	///   fails, falls back to 50,000.
	/// - Commitment type `2` (execution): uses a fixed default estimate of 100,000.
	/// - Other commitment types: returns an error.
	///
	/// # Returns
	///
	/// `Ok(u64)` with the estimated gas for the commitment, or an `Err` if the commitment type is unknown.
	///
	/// # Examples
	///
	/// ```no_run
	/// // Assuming `engine` is a FeePricingEngine instance
	/// let payload = vec![0u8; 128];
	/// let estimate = engine.estimate_gas_for_commitment(1, &payload).unwrap();
	/// assert!(estimate >= 21_000);
	/// ```
	fn estimate_gas_for_commitment(&self, commitment_type: u64, payload: &[u8]) -> Result<u64> {
		match commitment_type {
			1 => {
				// Inclusion commitment - parse the payload to extract signed_tx and derive gas limit
				use crate::types::payload::PayloadParser;

				match PayloadParser::parse_inclusion_payload(payload) {
					Ok(inclusion_payload) => {
						// The signed_tx is an RLP-encoded Ethereum transaction
						// For now, we'll use a simplified estimation based on the signed tx size
						// In production, you'd fully decode the transaction to get the actual gas_limit field

						let signed_tx = inclusion_payload.signed_tx();
						let base_gas = 21_000; // Base transaction cost
						let data_gas = signed_tx.len() as u64 * 16; // 16 gas per byte
						let estimation_overhead = 10_000; // Buffer for execution overhead

						let total_estimated = base_gas + data_gas + estimation_overhead;

						debug!(
							"Estimated gas for inclusion commitment with {} byte signed tx: {} (base) + {} (data) + {} (overhead) = {}",
							signed_tx.len(),
							base_gas,
							data_gas,
							estimation_overhead,
							total_estimated
						);

						Ok(total_estimated)
					}
					Err(_) => {
						// Fallback: if we can't parse, use a conservative estimate
						warn!("Could not parse inclusion payload, using fallback gas estimate");
						Ok(50_000)
					}
				}
			}
			2 => {
				// Execution commitment - would need more sophisticated analysis
				// For now, use a higher default estimate
				Ok(100_000)
			}
			_ => Err(anyhow::anyhow!("Unknown commitment type {} for gas estimation", commitment_type)),
		}
	}

	/// Calculate projected congestion if we add the specified gas
	fn calculate_projected_congestion(
		&self,
		current_congestion: &SlotCongestion,
		additional_gas: u64,
	) -> Result<SlotCongestion> {
		let mut projected = current_congestion.clone();
		projected.add_gas_usage(additional_gas, self.fee_config.scaling_factor);

		// Apply fee bounds from config
		projected.calculated_fee_multiplier = projected
			.calculated_fee_multiplier
			.max(self.fee_config.min_fee_multiplier)
			.min(self.fee_config.max_fee_multiplier);

		// Recalculate final price with bounds, rounding up to avoid undercharging
		// Use ceil() to ensure we never charge less than the multiplier indicates
		// (e.g., 1 wei * 1.5 = 1.5 wei should round up to 2 wei, not down to 1 wei)
		let scaled_price = (projected.base_gas_price as f64 * projected.calculated_fee_multiplier).ceil();
		projected.current_tx_price =
			if scaled_price.is_finite() && scaled_price <= u64::MAX as f64 { scaled_price as u64 } else { u64::MAX };

		Ok(projected)
	}

	/// Determine the current beacon-chain slot used for fee calculations.
	///
	/// Returns the current slot index as a `u64`.
	///
	/// # Examples
	///
	/// ```
	/// let engine = /* FeePricingEngine constructed elsewhere */;
	/// let slot = engine.get_current_slot();
	/// // `slot` is the current beacon-chain slot used when computing fees
	/// ```
	pub fn get_current_slot(&self) -> u64 {
		BeaconTiming::current_slot_estimate(self.config.beacon_api.genesis_time)
	}

	/// Determine whether a slot is within the acceptable window for fee calculation.
	///
	/// Acceptable slots are from the engine's current slot up to 10 slots ahead (inclusive).
	///
	/// # Returns
	///
	/// `true` if `slot` is greater than or equal to the current slot and less than or equal to the current slot plus 10, `false` otherwise.
	///
	/// # Examples
	///
	/// ```
	/// // `engine` is an instance of FeePricingEngine
	/// let current = engine.get_current_slot();
	/// assert!(engine.is_slot_acceptable_for_fees(current));
	/// assert!(engine.is_slot_acceptable_for_fees(current + 10));
	/// assert!(!engine.is_slot_acceptable_for_fees(current + 11));
	/// ```
	pub fn is_slot_acceptable_for_fees(&self, slot: u64) -> bool {
		let current_slot = self.get_current_slot();
		let max_lookahead = 10; // Allow fees for next 10 slots

		slot >= current_slot && slot <= current_slot + max_lookahead
	}

	/// Starts a background task that periodically refreshes the gas price cache.
	///
	/// The refresh interval is computed as half of the configured `cache_ttl_secs`, clamped to at least
	/// 1 second. The function spawns a background task to fetch the current gas price at that interval
	/// and logs refresh successes and failures. Returns once the background task has been started.
	///
	/// # Examples
	///
	/// ```
	/// # tokio_test::block_on(async {
	/// let engine = /* construct FeePricingEngine */ unimplemented!();
	/// engine.start_cache_refresh_service().await.unwrap();
	/// # });
	/// ```
	pub async fn start_cache_refresh_service(&self) -> Result<()> {
		info!("Starting fee pricing cache refresh service");

		// Clamp refresh interval to at least 1 second to avoid panic from Duration::from_secs(0)
		let refresh_interval_secs = (self.fee_config.cache_ttl_secs / 2).max(1);
		let refresh_interval = Duration::from_secs(refresh_interval_secs);
		let reth_client = Arc::clone(&self.reth_client);

		tokio::spawn(async move {
			let mut interval = tokio::time::interval(refresh_interval);

			loop {
				interval.tick().await;

				match reth_client.get_gas_price().await {
					Ok(gas_price_info) => {
						debug!(
							"Refreshed gas price cache: {} wei at block {}",
							gas_price_info.gas_price, gas_price_info.block_number
						);
						// TODO: Store in actual cache when implemented
					}
					Err(e) => {
						warn!("Failed to refresh gas price cache: {}", e);
					}
				}
			}
		});

		Ok(())
	}

	/// Return current pricing metrics for monitoring and observability.
	///
	/// The returned `PricingStats` contains the engine's current slot estimate, an optional
	/// best-effort base gas price, and aggregated congestion/fee-multiplier statistics
	/// computed from persisted slot congestion data. `current_base_gas_price` will be
	/// `None` if the gas-price fetch fails.
	///
	/// # Returns
	///
	/// A `PricingStats` struct populated with the current slot, optional base gas price,
	/// average congestion ratio, average fee multiplier, total tracked slots, and the
	/// highest-observed congestion slot and ratio.
	///
	/// # Examples
	///
	/// ```
	/// # use std::sync::Arc;
	/// # use tokio::runtime::Runtime;
	/// # async fn _example(engine: Arc<crate::services::fee_pricing::FeePricingEngine>) {
	/// let stats = engine.get_pricing_stats().await.unwrap();
	/// // inspect returned metrics
	/// let _current_slot = stats.current_slot;
	/// # }
	/// ```
	pub async fn get_pricing_stats(&self) -> Result<PricingStats> {
		let congestion_stats = self.database.get_congestion_stats().await?;
		let current_slot = self.get_current_slot();

		let current_gas_price = match self.get_cached_gas_price().await {
			Ok(info) => Some(info.gas_price_as_u64_clamped()),
			Err(_) => None,
		};

		Ok(PricingStats {
			current_slot,
			current_base_gas_price: current_gas_price,
			average_congestion_ratio: congestion_stats.current_average_congestion,
			average_fee_multiplier: congestion_stats.average_fee_multiplier,
			total_slots_tracked: congestion_stats.total_slots_tracked,
			highest_congestion_slot: congestion_stats.highest_congestion_slot,
			highest_congestion_ratio: congestion_stats.highest_congestion_ratio,
		})
	}
}

/// Pricing statistics for monitoring
#[derive(Debug, Clone)]
pub struct PricingStats {
	pub current_slot: u64,
	pub current_base_gas_price: Option<u64>,
	pub average_congestion_ratio: f64,
	pub average_fee_multiplier: f64,
	pub total_slots_tracked: u64,
	pub highest_congestion_slot: Option<u64>,
	pub highest_congestion_ratio: f64,
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::reth::RethApiConfig;
	use crate::config::Config;

	/// Verifies that gas estimation for inclusion commitments scales with payload size and returns values within expected bounds.
	///
	/// # Examples
	///
	/// ```
	/// // Small payload should require more than base tx cost but remain reasonable
	/// let small_payload = vec![0u8; 100];
	/// let gas = engine.estimate_gas_for_commitment(1, &small_payload).unwrap();
	/// assert!(gas > 21_000);
	/// assert!(gas < 100_000);
	///
	/// // Larger payload should not produce a smaller estimate than the smaller payload
	/// let large_payload = vec![0u8; 1000];
	/// let large_gas = engine.estimate_gas_for_commitment(1, &large_payload).unwrap();
	/// assert!(large_gas >= gas);
	/// ```
	#[tokio::test]
	async fn test_gas_estimation_inclusion() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		// Test small payload
		let small_payload = vec![0u8; 100]; // 100 bytes
		let gas = engine.estimate_gas_for_commitment(1, &small_payload).unwrap();
		assert!(gas > 21_000); // Should be more than base transaction cost
		assert!(gas < 100_000); // Should be reasonable for small payload

		// Test larger payload
		let large_payload = vec![0u8; 1000]; // 1000 bytes
		let large_gas = engine.estimate_gas_for_commitment(1, &large_payload).unwrap();
		// Both should give reasonable estimates (fallback uses 50,000 when parsing fails)
		assert!(large_gas >= gas); // Should be at least equal or higher for larger payload
	}

	#[tokio::test]
	async fn test_slot_acceptability() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		let current_slot = engine.get_current_slot();

		// Current slot should be acceptable
		assert!(engine.is_slot_acceptable_for_fees(current_slot));

		// Near future slots should be acceptable
		assert!(engine.is_slot_acceptable_for_fees(current_slot + 5));

		// Far future slots should not be acceptable
		assert!(!engine.is_slot_acceptable_for_fees(current_slot + 20));

		// Past slots should not be acceptable
		if current_slot > 0 {
			assert!(!engine.is_slot_acceptable_for_fees(current_slot - 1));
		}
	}

	#[tokio::test]
	async fn test_projected_congestion_calculation() {
		let config = Config::default();
		let reth_client = Arc::new(RethApiClient::new(RethApiConfig::default()).unwrap());
		let database = Arc::new(DatabaseContext::new_for_testing());

		let engine = FeePricingEngine::new(reth_client, database, Arc::new(config));

		let mut base_congestion = SlotCongestion::new(
			12345,
			1_000_000_000, // 1 gwei
			30_000_000,    // 30M gas limit
			std::time::SystemTime::now(),
		);

		// Add some initial usage (25% of limit)
		base_congestion.add_gas_usage(7_500_000, 2.0);

		// Project adding another 25% (total 50%)
		let projected = engine.calculate_projected_congestion(&base_congestion, 7_500_000).unwrap();

		assert_eq!(projected.gas_used_ratio, 0.5);
		assert!(projected.calculated_fee_multiplier > base_congestion.calculated_fee_multiplier);
		assert!(projected.current_tx_price > base_congestion.current_tx_price);
	}
}
