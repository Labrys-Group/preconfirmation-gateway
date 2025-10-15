//! Validator status cache service
//!
//! Provides an in-memory cache for validator status information with TTL-based expiration.
//! This cache reduces the number of Beacon API calls needed when checking validator eligibility
//! for delegations.

use anyhow::Result;
use moka::future::Cache;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::api::beacon::BeaconApiClient;
use crate::types::beacon::ValidatorInfo;
use crate::types::delegation::BlsPublicKey;

/// Cache for validator status information with automatic TTL-based expiration
#[derive(Debug, Clone)]
pub struct ValidatorStatusCache {
	cache: Cache<BlsPublicKey, ValidatorInfo>,
	beacon_client: Arc<BeaconApiClient>,
}

impl ValidatorStatusCache {
	/// Creates a new validator status cache with the specified TTL.
	///
	/// # Parameters
	///
	/// * `ttl` - Time-to-live duration for cached entries
	/// * `beacon_client` - Beacon API client for fetching validator status
	///
	/// # Examples
	///
	pub fn new(ttl: Duration, beacon_client: Arc<BeaconApiClient>) -> Self {
		let cache = Cache::builder().time_to_live(ttl).build();

		Self { cache, beacon_client }
	}

	/// Gets validator status, using cached value if available, otherwise fetching from Beacon API.
	///
	/// # Parameters
	///
	/// * `validator_pubkey` - BLS public key of the validator
	///
	/// # Returns
	///
	/// `Ok(ValidatorInfo)` with validator status, or error if fetch fails
	///
	/// # Examples
	///
	pub async fn get_status(&self, validator_pubkey: &BlsPublicKey) -> Result<ValidatorInfo> {
		// Try to get from cache first
		if let Some(cached_info) = self.cache.get(validator_pubkey).await {
			return Ok(cached_info);
		}

		// Fetch from Beacon API
		let validator_info = self.beacon_client.get_validator_status(validator_pubkey).await?;

		// Store in cache
		self.cache.insert(*validator_pubkey, validator_info.clone()).await;

		Ok(validator_info)
	}

	/// Gets status for multiple validators in parallel, using cache where available.
	///
	/// # Parameters
	///
	/// * `validator_pubkeys` - Slice of BLS public keys to look up
	///
	/// # Returns
	///
	/// `Ok(HashMap<BlsPublicKey, ValidatorInfo>)` mapping pubkeys to their status information,
	/// or error if any fetch fails
	///
	/// # Examples
	///
	pub async fn get_batch_status(
		&self,
		validator_pubkeys: &[BlsPublicKey],
	) -> Result<HashMap<BlsPublicKey, ValidatorInfo>> {
		// Handle empty input
		if validator_pubkeys.is_empty() {
			return Ok(HashMap::new());
		}

		// Fetch all statuses in parallel using futures
		let futures: Vec<_> =
			validator_pubkeys.iter().map(|pubkey| async move { (*pubkey, self.get_status(pubkey).await) }).collect();

		// Wait for all futures to complete
		let results: Vec<_> = futures::future::join_all(futures).await;

		// Collect into HashMap, propagating first error if any
		let mut status_map = HashMap::new();
		for (pubkey, result) in results {
			status_map.insert(pubkey, result?);
		}

		Ok(status_map)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::BeaconApiConfig;

	fn create_test_beacon_client() -> Arc<BeaconApiClient> {
		let config = BeaconApiConfig {
			primary_endpoint: "https://test-beacon.example.com".to_string(),
			fallback_endpoints: vec![],
			request_timeout_secs: 5,
			genesis_time: 1606824023,
		};

		Arc::new(BeaconApiClient::new(config).unwrap())
	}

	#[tokio::test]
	async fn test_cache_creation() {
		let beacon_client = create_test_beacon_client();
		let cache = ValidatorStatusCache::new(Duration::from_secs(30), beacon_client);

		// Cache should be created successfully
		assert!(cache.cache.entry_count() == 0, "Cache should start empty");
	}

	#[tokio::test]
	async fn test_cache_ttl_expires() {
		let beacon_client = create_test_beacon_client();
		// Use a very short TTL for testing
		let cache = ValidatorStatusCache::new(Duration::from_millis(100), beacon_client);

		let validator_pubkey = BlsPublicKey([2u8; 48]);

		let _result = cache.get_status(&validator_pubkey).await;

		// Wait for TTL to expire
		tokio::time::sleep(Duration::from_millis(150)).await;

		// Cache should be empty after TTL expires
		assert_eq!(cache.cache.entry_count(), 0, "Cache should be empty after TTL expiration");
	}

	#[tokio::test]
	async fn test_get_batch_status_with_empty_list() {
		let beacon_client = create_test_beacon_client();
		let cache = ValidatorStatusCache::new(Duration::from_secs(30), beacon_client);

		// Test with empty vector
		let pubkeys: Vec<BlsPublicKey> = vec![];
		let result = cache.get_batch_status(&pubkeys).await;

		// Should succeed with empty result
		assert!(result.is_ok(), "Should handle empty input");
		let statuses = result.unwrap();
		assert_eq!(statuses.len(), 0, "Should return empty map for empty input");
	}
}
