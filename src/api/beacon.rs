//! Beacon API client for retrieving proposer duties and slot information
//!
//! This module provides integration with Ethereum Beacon Chain API endpoints,
//! specifically designed to work with Alchemy's Beacon API or any compatible
//! beacon node endpoint.

use anyhow::{Context, Result};
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, warn};

use crate::config::BeaconApiConfig;
use crate::types::beacon::{BeaconTiming, ProposerDutiesResponse, ValidatorDuty};

/// Beacon API client for retrieving chain state and proposer information
#[derive(Debug, Clone)]
pub struct BeaconApiClient {
	client: Client,
	config: BeaconApiConfig,
}

impl BeaconApiClient {
	/// Create a new Beacon API client
	pub fn new(config: BeaconApiConfig) -> Result<Self> {
		let client = Client::builder()
			.timeout(Duration::from_secs(config.request_timeout_secs))
			.build()
			.context("Failed to create HTTP client")?;

		Ok(Self { client, config })
	}

	/// Retrieve proposer duties for a specific epoch
	///
	/// This is the primary method for getting scheduled proposers, which is essential
	/// for delegation verification and constraint targeting.
	pub async fn get_proposer_duties(&self, epoch: u64) -> Result<ProposerDutiesResponse> {
		let endpoint = format!("eth/v1/validator/duties/proposer/{}", epoch);

		// Try primary endpoint first, then fallbacks
		let mut _last_error = None;

		// Try primary endpoint
		match self.make_request(&self.config.primary_endpoint, &endpoint).await {
			Ok(response) => return Ok(response),
			Err(e) => {
				warn!(
					endpoint = %self.config.primary_endpoint,
					epoch = epoch,
					error = %e,
					"Primary beacon endpoint failed, trying fallbacks"
				);
				_last_error = Some(e);
			}
		}

		// Try fallback endpoints
		for fallback_endpoint in &self.config.fallback_endpoints {
			match self.make_request(fallback_endpoint, &endpoint).await {
				Ok(response) => {
					debug!(
						endpoint = %fallback_endpoint,
						epoch = epoch,
						"Successfully retrieved proposer duties from fallback endpoint"
					);
					return Ok(response);
				}
				Err(e) => {
					warn!(
						endpoint = %fallback_endpoint,
						epoch = epoch,
						error = %e,
						"Fallback beacon endpoint failed"
					);
					_last_error = Some(e);
				}
			}
		}

		// All endpoints failed
		Err(_last_error.unwrap_or_else(|| anyhow::anyhow!("No beacon endpoints configured")))
	}

	/// Get proposer for a specific slot
	pub async fn get_proposer_for_slot(&self, slot: u64) -> Result<Option<ValidatorDuty>> {
		let epoch = BeaconTiming::slot_to_epoch(slot);
		let duties = self.get_proposer_duties(epoch).await?;

		// Find the duty for the specific slot
		Ok(duties.data.into_iter().find(|duty| {
			duty.parse_slot().unwrap_or(0) == slot
		}))
	}

	/// Internal method to make HTTP requests with error handling
	async fn make_request<T>(&self, base_url: &str, endpoint: &str) -> Result<T>
	where
		T: for<'de> Deserialize<'de>,
	{
		let url = if base_url.ends_with('/') {
			format!("{}{}", base_url, endpoint)
		} else {
			format!("{}/{}", base_url, endpoint)
		};

		debug!(url = %url, "Making beacon API request");

		let request = self.client.get(&url);
		let response = self.add_headers(request)
			.send()
			.await
			.with_context(|| format!("Failed to send request to {}", url))?;

		if !response.status().is_success() {
			let status = response.status();
			let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
			anyhow::bail!("Beacon API request failed with status {}: {}", status, error_text);
		}

		let result: T = response
			.json()
			.await
			.with_context(|| format!("Failed to parse response from {}", url))?;

		Ok(result)
	}

	/// Add necessary headers to the request
	fn add_headers(&self, request: RequestBuilder) -> RequestBuilder {
		request
			.header("Content-Type", "application/json")
			.header("User-Agent", "preconfirmation-gateway/0.1.0")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::BeaconApiConfig;

	fn create_test_config() -> BeaconApiConfig {
		BeaconApiConfig {
			primary_endpoint: "https://eth-mainnet.g.alchemy.com/v2/test".to_string(),
			fallback_endpoints: vec![
				"https://beacon-nd-123-456-789.p2pify.com".to_string()
			],
			request_timeout_secs: 30,
			genesis_time: 1606824023, // Ethereum mainnet genesis
		}
	}

	#[test]
	fn test_client_creation() {
		let config = create_test_config();
		let client = BeaconApiClient::new(config);
		assert!(client.is_ok());
	}

	#[test]
	fn test_epoch_calculation() {
		let config = create_test_config();
		let _client = BeaconApiClient::new(config).unwrap();

		// This test would need to be updated with actual network calls for integration testing
		// For now, just verify the client can be created
	}

	// Integration tests would go here, requiring actual beacon endpoints
	// These should be marked with #[ignore] or put behind a feature flag
}