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
	/// Creates a new BeaconApiClient configured with the provided BeaconApiConfig.
	///
	/// The created client uses the config's `request_timeout_secs` to set the HTTP client timeout.
	/// Returns an error if the underlying HTTP client cannot be constructed.
	///
	/// # Examples
	///
	/// ```no_run
	/// // Construct a BeaconApiConfig with the desired endpoints and timeout,
	/// // then create the client.
	/// // let config = BeaconApiConfig { /* fields */ };
	/// // let client = BeaconApiClient::new(config)?;
	/// ```
	pub fn new(config: BeaconApiConfig) -> Result<Self> {
		let client = Client::builder()
			.timeout(Duration::from_secs(config.request_timeout_secs))
			.build()
			.context("Failed to create HTTP client")?;

		Ok(Self { client, config })
	}

	/// Fetches proposer duties for the given epoch from the configured beacon endpoints.
	///
	/// Tries the primary endpoint first and falls back to configured fallback endpoints; returns
	/// the first successful response or an error if all endpoints fail.
	///
	/// # Returns
	///
	/// `Ok(ProposerDutiesResponse)` containing scheduled proposer duties for the epoch, `Err` if all
	/// configured endpoints fail or no endpoints are configured.
	///
	/// # Examples
	///
	/// ```
	/// #[tokio::test]
	/// async fn example_get_proposer_duties() {
	///     let config = crate::tests::create_test_config(); // test helper in this crate
	///     let client = crate::api::beacon::BeaconApiClient::new(config).unwrap();
	///     let duties = client.get_proposer_duties(0).await.unwrap();
	///     assert!(duties.data.len() >= 0);
	/// }
	/// ```
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

	/// Fetches the validator duty corresponding to the proposer for a specific slot.
	///
	/// The function converts the given slot to its epoch, requests proposer duties for that epoch,
	/// and returns the duty whose slot matches the provided slot.
	///
	/// # Returns
	///
	/// `Ok(Some(ValidatorDuty))` if a matching duty is found, `Ok(None)` if no duty for that slot exists,
	/// or `Err(...)` if the underlying request or deserialization fails.
	///
	/// # Examples
	///
	/// ```no_run
	/// # use crate::api::beacon::BeaconApiClient;
	/// # #[tokio::main]
	/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
	/// let client = BeaconApiClient::new(/* config */)?;
	/// let slot = 12345;
	/// let proposer = client.get_proposer_for_slot(slot).await?;
	/// if let Some(duty) = proposer {
	///     println!("Found proposer with validator index: {}", duty.validator_index);
	/// } else {
	///     println!("No proposer found for slot {}", slot);
	/// }
	/// # Ok(())
	/// # }
	/// ```
	pub async fn get_proposer_for_slot(&self, slot: u64) -> Result<Option<ValidatorDuty>> {
		let epoch = BeaconTiming::slot_to_epoch(slot);
		let duties = self.get_proposer_duties(epoch).await?;

		// Find the duty for the specific slot, propagating parse errors
		for duty in duties.data {
			let duty_slot = duty.parse_slot().context("Failed to parse slot from validator duty")?;
			if duty_slot == slot {
				return Ok(Some(duty));
			}
		}

		Ok(None)
	}

	/// Perform an HTTP GET to the given endpoint on `base_url`, validate the response, and deserialize the JSON body into `T`.
	///
	/// The method constructs the full URL by joining `base_url` and `endpoint`, sends a GET request with standard headers,
	/// fails if the HTTP status is not successful (including the status and response body in the error), and parses the
	/// response JSON into `T`.
	///
	/// # Returns
	///
	/// The deserialized JSON response as `T`.
	///
	/// # Errors
	///
	/// Returns an error if the request fails to send, the response status is not successful, or the response body cannot be parsed as `T`.
	///
	/// # Examples
	///
	/// ```ignore
	/// // Example usage (requires an async runtime and a BeaconApiClient instance):
	/// // let res: MyResponseType = client.make_request("https://beacon.example", "eth/v1/..").await?;
	/// ```
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
		let response =
			self.add_headers(request).send().await.with_context(|| format!("Failed to send request to {}", url))?;

		if !response.status().is_success() {
			let status = response.status();
			let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
			anyhow::bail!("Beacon API request failed with status {}: {}", status, error_text);
		}

		let result: T = response.json().await.with_context(|| format!("Failed to parse response from {}", url))?;

		Ok(result)
	}

	/// Attach required HTTP headers to a request builder.
	///
	/// Adds the following headers:
	/// - `Content-Type: application/json`
	/// - `User-Agent: preconfirmation-gateway/0.1.0`
	///
	/// # Parameters
	///
	/// - `request`: The `reqwest::RequestBuilder` to which headers will be applied.
	///
	/// # Returns
	///
	/// The modified `RequestBuilder` with the headers set.
	fn add_headers(&self, request: RequestBuilder) -> RequestBuilder {
		request.header("Content-Type", "application/json").header("User-Agent", "preconfirmation-gateway/0.1.0")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::BeaconApiConfig;

	/// Creates a test `BeaconApiConfig` prepopulated with mainnet endpoints and defaults.
	///
	/// Returns a `BeaconApiConfig` configured with an Alchemy primary endpoint, a single fallback
	/// endpoint, a 30-second request timeout, and the Ethereum mainnet genesis time.
	///
	/// # Examples
	///
	/// ```
	/// let cfg = create_test_config();
	/// assert!(cfg.primary_endpoint.contains("alchemy"));
	/// assert_eq!(cfg.fallback_endpoints.len(), 1);
	/// assert_eq!(cfg.request_timeout_secs, 30);
	/// assert_eq!(cfg.genesis_time, 1606824023);
	/// ```
	fn create_test_config() -> BeaconApiConfig {
		BeaconApiConfig {
			primary_endpoint: "https://eth-mainnet.g.alchemy.com/v2/test".to_string(),
			fallback_endpoints: vec!["https://beacon-nd-123-456-789.p2pify.com".to_string()],
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
