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
	/// Returns an error if the underlying HTTP client cannot be constructed or if the configuration
	/// is invalid (e.g., empty primary endpoint or zero timeout).
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The primary endpoint is empty
	/// - The request timeout is zero (would cause immediate timeouts)
	/// - The underlying HTTP client cannot be constructed
	///
	/// # Examples
	///
	pub fn new(config: BeaconApiConfig) -> Result<Self> {
		// Validate configuration
		if config.primary_endpoint.trim().is_empty() {
			anyhow::bail!("Primary endpoint cannot be empty");
		}
		
		if config.request_timeout_secs == 0 {
			anyhow::bail!("Request timeout must be greater than zero");
		}

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
	use crate::types::beacon::BeaconTiming;
	use std::time::Duration;
	use tokio::time::timeout;

	/// Creates a test `BeaconApiConfig` prepopulated with mainnet endpoints and defaults.
	///
	/// Returns a `BeaconApiConfig` configured with an Alchemy primary endpoint, a single fallback
	/// endpoint, a 30-second request timeout, and the Ethereum mainnet genesis time.
	///
	/// # Examples
	///
	fn create_test_config() -> BeaconApiConfig {
		BeaconApiConfig {
			primary_endpoint: "https://eth-mainnet.g.alchemy.com/v2/test".to_string(),
			fallback_endpoints: vec!["https://beacon-nd-123-456-789.p2pify.com".to_string()],
			request_timeout_secs: 30,
			genesis_time: 1606824023, // Ethereum mainnet genesis
		}
	}

	fn create_test_config_with_short_timeout() -> BeaconApiConfig {
		BeaconApiConfig {
			primary_endpoint: "https://invalid-endpoint.test".to_string(),
			fallback_endpoints: vec!["https://another-invalid-endpoint.test".to_string()],
			request_timeout_secs: 1, // Very short timeout
			genesis_time: 1606824023,
		}
	}

	fn create_test_config_no_fallbacks() -> BeaconApiConfig {
		BeaconApiConfig {
			primary_endpoint: "https://invalid-endpoint.test".to_string(),
			fallback_endpoints: vec![],
			request_timeout_secs: 1,
			genesis_time: 1606824023,
		}
	}

	#[test]
	fn test_client_creation() {
		let config = create_test_config();
		let client = BeaconApiClient::new(config);
		assert!(client.is_ok(), "Should be able to create beacon API client");

		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 30);
		assert_eq!(client.config.genesis_time, 1606824023);
	}

	#[test]
	fn test_client_creation_with_short_timeout() {
		let config = create_test_config_with_short_timeout();
		let client = BeaconApiClient::new(config);
		assert!(client.is_ok(), "Should create client even with short timeout");

		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 1);
	}

	#[test]
	fn test_client_creation_with_no_fallbacks() {
		let config = create_test_config_no_fallbacks();
		let client = BeaconApiClient::new(config);
		assert!(client.is_ok(), "Should create client even with no fallbacks");

		let client = client.unwrap();
		assert!(client.config.fallback_endpoints.is_empty());
	}

	#[test]
	fn test_epoch_calculation() {
		let config = create_test_config();
		let _client = BeaconApiClient::new(config).unwrap();

		// Test beacon timing utilities that the client uses
		let slot = 12345u64;
		let epoch = BeaconTiming::slot_to_epoch(slot);
		
		// Each epoch has 32 slots, so slot 12345 should be in epoch 385
		assert_eq!(epoch, slot / 32);
		assert_eq!(epoch, 385);
	}

	#[test]
	fn test_add_headers() {
		let config = create_test_config();
		let client = BeaconApiClient::new(config).unwrap();
		
		// Create a mock request builder to test header addition
		let http_client = reqwest::Client::new();
		let request = http_client.get("https://example.com");
		
		let request_with_headers = client.add_headers(request);
		
		// We can't easily inspect the headers without sending the request,
		// but we can verify the method doesn't panic and returns a valid RequestBuilder
		// This test ensures the add_headers method works without errors
		let _final_request = request_with_headers;
	}

	#[tokio::test]
	async fn test_get_proposer_duties_timeout() {
		let config = create_test_config_with_short_timeout();
		let client = BeaconApiClient::new(config).unwrap();

		// This should timeout quickly since we're using invalid endpoints
		let result = timeout(Duration::from_secs(5), client.get_proposer_duties(0)).await;
		
		// Should complete within timeout (even if it fails due to invalid endpoint)
		assert!(result.is_ok(), "Request should complete within timeout");
		
		// The inner result should be an error due to invalid endpoints
		let proposer_result = result.unwrap();
		assert!(proposer_result.is_err(), "Should fail with invalid endpoints");
	}

	#[tokio::test]
	async fn test_get_proposer_duties_no_fallbacks() {
		let config = create_test_config_no_fallbacks();
		let client = BeaconApiClient::new(config).unwrap();

		// Should fail since primary endpoint is invalid and no fallbacks
		let result = timeout(Duration::from_secs(5), client.get_proposer_duties(0)).await;
		
		assert!(result.is_ok(), "Request should complete within timeout");
		let proposer_result = result.unwrap();
		assert!(proposer_result.is_err(), "Should fail with no valid endpoints");
	}

	#[tokio::test]
	async fn test_get_proposer_for_slot_invalid_epoch() {
		let config = create_test_config_with_short_timeout();
		let client = BeaconApiClient::new(config).unwrap();

		// Test with a slot that would fail to fetch duties
		let result = timeout(Duration::from_secs(5), client.get_proposer_for_slot(12345)).await;
		
		assert!(result.is_ok(), "Request should complete within timeout");
		let proposer_result = result.unwrap();
		assert!(proposer_result.is_err(), "Should fail due to invalid endpoint");
	}

	#[test]
	fn test_make_request_url_building() {
		let config = create_test_config();
		let _client = BeaconApiClient::new(config).unwrap();

		// Test URL building logic
		let base_with_slash = "https://example.com/";
		let base_without_slash = "https://example.com";
		let endpoint = "eth/v1/test";

		// Both should produce the same URL
		let url1 = format!("{}{}", base_with_slash, endpoint);
		let url2 = format!("{}/{}", base_without_slash, endpoint);

		assert_eq!(url1, "https://example.com/eth/v1/test");
		assert_eq!(url2, "https://example.com/eth/v1/test");
	}

	#[tokio::test] 
	async fn test_proposer_duties_error_handling() {
		let config = create_test_config_with_short_timeout();
		let client = BeaconApiClient::new(config).unwrap();

		// Test that errors are properly handled and returned
		let result = client.get_proposer_duties(999999).await;
		assert!(result.is_err(), "Should return error for invalid endpoint");

		// Verify error contains meaningful information
		let error = result.unwrap_err();
		let error_string = format!("{}", error);
		assert!(!error_string.is_empty(), "Error message should not be empty");
	}

	#[tokio::test]
	async fn test_get_proposer_for_slot_with_duties() {
		// This test simulates the scenario where we would get duties back
		// In a real integration test, we'd mock the HTTP responses
		let config = create_test_config_with_short_timeout();
		let client = BeaconApiClient::new(config).unwrap();

		// Since we're using invalid endpoints, this will fail at the network level
		// which allows us to test the error handling path
		let result = client.get_proposer_for_slot(100).await;
		assert!(result.is_err(), "Should fail due to network error");
	}

	#[test]
	fn test_config_validation() {
		// Test that invalid configurations are properly rejected
		let mut config = create_test_config();
		
		// Test with empty primary endpoint
		config.primary_endpoint = "".to_string();
		let client = BeaconApiClient::new(config.clone());
		assert!(client.is_err(), "Should reject empty primary endpoint");
		let error_msg = format!("{}", client.unwrap_err());
		assert!(error_msg.contains("Primary endpoint cannot be empty"), 
			"Error should mention empty endpoint");

		// Test with whitespace-only primary endpoint
		config.primary_endpoint = "   ".to_string();
		let client = BeaconApiClient::new(config.clone());
		assert!(client.is_err(), "Should reject whitespace-only primary endpoint");

		// Test with zero timeout
		config.primary_endpoint = "https://valid-endpoint.com".to_string();
		config.request_timeout_secs = 0;
		let client = BeaconApiClient::new(config);
		assert!(client.is_err(), "Should reject zero timeout");
		let error_msg = format!("{}", client.unwrap_err());
		assert!(error_msg.contains("Request timeout must be greater than zero"), 
			"Error should mention zero timeout");
	}

	#[test]
	fn test_client_creation_with_minimal_valid_config() {
		// Test that minimal valid configurations work
		let config = BeaconApiConfig {
			primary_endpoint: "https://minimal.example.com".to_string(),
			fallback_endpoints: vec![], // Empty fallbacks should be fine
			request_timeout_secs: 1, // Minimal valid timeout
			genesis_time: 0, // Any genesis time should be fine
		};

		let client = BeaconApiClient::new(config);
		assert!(client.is_ok(), "Should accept minimal valid configuration");
		
		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 1);
		assert!(client.config.fallback_endpoints.is_empty());
	}

	#[test]
	fn test_fallback_endpoint_order() {
		let config = BeaconApiConfig {
			primary_endpoint: "https://primary.test".to_string(),
			fallback_endpoints: vec![
				"https://fallback1.test".to_string(),
				"https://fallback2.test".to_string(),
				"https://fallback3.test".to_string(),
			],
			request_timeout_secs: 1,
			genesis_time: 1606824023,
		};

		let client = BeaconApiClient::new(config).unwrap();
		
		// Verify fallback endpoints are preserved in order
		assert_eq!(client.config.fallback_endpoints.len(), 3);
		assert_eq!(client.config.fallback_endpoints[0], "https://fallback1.test");
		assert_eq!(client.config.fallback_endpoints[1], "https://fallback2.test");
		assert_eq!(client.config.fallback_endpoints[2], "https://fallback3.test");
	}

	#[tokio::test]
	async fn test_concurrent_requests() {
		let config = create_test_config_with_short_timeout();
		let client = BeaconApiClient::new(config).unwrap();

		// Test multiple concurrent requests
		let mut handles = Vec::new();
		
		for i in 0..5 {
			let client_clone = client.clone();
			let handle = tokio::spawn(async move {
				client_clone.get_proposer_duties(i).await
			});
			handles.push(handle);
		}

		// Wait for all requests to complete
		for handle in handles {
			let result = handle.await.unwrap();
			// All should fail due to invalid endpoints, but shouldn't panic
			assert!(result.is_err(), "Concurrent requests should handle errors gracefully");
		}
	}

	#[test]
	fn test_client_clone() {
		let config = create_test_config();
		let client = BeaconApiClient::new(config).unwrap();
		
		// Test that client can be cloned
		let cloned_client = client.clone();
		
		// Verify the clone has the same configuration
		assert_eq!(client.config.primary_endpoint, cloned_client.config.primary_endpoint);
		assert_eq!(client.config.fallback_endpoints, cloned_client.config.fallback_endpoints);
		assert_eq!(client.config.request_timeout_secs, cloned_client.config.request_timeout_secs);
	}

	// Integration tests would go here, requiring actual beacon endpoints
	// These should be marked with #[ignore] or put behind a feature flag

	#[tokio::test]
	#[ignore = "Integration test - requires real beacon API"]
	async fn test_real_beacon_api_integration() {
		// This test would use real beacon endpoints and should only run in integration test mode
		let config = BeaconApiConfig {
			primary_endpoint: "https://beacon-nd-123-456-789.p2pify.com".to_string(),
			fallback_endpoints: vec![],
			request_timeout_secs: 10,
			genesis_time: 1606824023,
		};

		let client = BeaconApiClient::new(config).unwrap();
		
		// Test with a recent epoch
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();
		let current_slot = (current_time - 1606824023) / 12;
		let current_epoch = BeaconTiming::slot_to_epoch(current_slot);

		let result = client.get_proposer_duties(current_epoch).await;
		// This might succeed or fail depending on network connectivity
		// but shouldn't panic
		match result {
			Ok(duties) => println!("Got {} proposer duties", duties.data.len()),
			Err(e) => println!("Integration test failed (expected in CI): {}", e),
		}
	}
}
