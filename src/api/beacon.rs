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
use crate::types::beacon::{
	BeaconTiming, ProposerDutiesResponse, ValidatorData, ValidatorDuty, ValidatorInfo, ValidatorResponse,
};
use crate::types::delegation::BlsPublicKey;

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

	/// Fetches validator status information from the beacon chain.
	///
	/// Queries the Beacon API for the validator's current status, including whether
	/// they are active and whether they have been slashed.
	///
	/// # Parameters
	///
	/// * `validator_pubkey` - The BLS public key of the validator to query
	///
	/// # Returns
	///
	/// `Ok(ValidatorInfo)` containing the validator's status information, or an error
	/// if the request fails or the response cannot be parsed.
	///
	/// # Examples
	///
	pub async fn get_validator_status(&self, validator_pubkey: &BlsPublicKey) -> Result<ValidatorInfo> {
		// Format pubkey as hex string with 0x prefix
		let pubkey_hex = format!("0x{}", hex::encode(validator_pubkey.0));
		let endpoint = format!("eth/v1/beacon/states/head/validators/{}", pubkey_hex);

		// Try primary endpoint first, then fallbacks
		let mut _last_error = None;

		// Try primary endpoint
		match self.make_request::<ValidatorResponse>(&self.config.primary_endpoint, &endpoint).await {
			Ok(response) => return Self::parse_validator_info(&response.data),
			Err(e) => {
				warn!(
					endpoint = %self.config.primary_endpoint,
					pubkey = %pubkey_hex,
					error = %e,
					"Primary beacon endpoint failed for validator status, trying fallbacks"
				);
				_last_error = Some(e);
			}
		}

		// Try fallback endpoints
		for fallback_endpoint in &self.config.fallback_endpoints {
			match self.make_request::<ValidatorResponse>(fallback_endpoint, &endpoint).await {
				Ok(response) => {
					debug!(
						endpoint = %fallback_endpoint,
						pubkey = %pubkey_hex,
						"Successfully retrieved validator status from fallback endpoint"
					);
					return Self::parse_validator_info(&response.data);
				}
				Err(e) => {
					warn!(
						endpoint = %fallback_endpoint,
						pubkey = %pubkey_hex,
						error = %e,
						"Fallback beacon endpoint failed for validator status"
					);
					_last_error = Some(e);
				}
			}
		}

		// All endpoints failed
		Err(_last_error.unwrap_or_else(|| anyhow::anyhow!("No beacon endpoints configured")))
	}

	/// Parse validator data from the Beacon API into ValidatorInfo.
	///
	/// Determines if the validator is active based on their status string and extracts
	/// slashing status and validator index.
	///
	/// # Parameters
	///
	/// * `data` - Validator data from the Beacon API response
	///
	/// # Returns
	///
	/// `Ok(ValidatorInfo)` with parsed status information, or an error if parsing fails.
	fn parse_validator_info(data: &ValidatorData) -> Result<ValidatorInfo> {
		// Parse validator index
		let validator_index = data.index.parse::<u64>().context("Failed to parse validator index")?;

		// Determine if validator is active
		// According to spec: active_ongoing, active_exiting, or active_slashed
		let is_active = matches!(data.status.as_str(), "active_ongoing" | "active_exiting" | "active_slashed");

		// Get slashed status from validator details
		let is_slashed = data.validator.slashed;

		Ok(ValidatorInfo { is_active, is_slashed, validator_index })
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

	#[test]
	fn test_config_validation() {
		// Test that invalid configurations are properly rejected
		let mut config = create_test_config();

		// Test with empty primary endpoint
		config.primary_endpoint = "".to_string();
		let client = BeaconApiClient::new(config.clone());
		assert!(client.is_err(), "Should reject empty primary endpoint");
		let error_msg = format!("{}", client.unwrap_err());
		assert!(error_msg.contains("Primary endpoint cannot be empty"), "Error should mention empty endpoint");

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
		assert!(error_msg.contains("Request timeout must be greater than zero"), "Error should mention zero timeout");
	}

	#[test]
	fn test_client_creation_with_minimal_valid_config() {
		// Test that minimal valid configurations work
		let config = BeaconApiConfig {
			primary_endpoint: "https://minimal.example.com".to_string(),
			fallback_endpoints: vec![], // Empty fallbacks should be fine
			request_timeout_secs: 1,    // Minimal valid timeout
			genesis_time: 0,            // Any genesis time should be fine
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
		let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
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

	// ========================================
	// Tests for parse_validator_info
	// ========================================

	#[test]
	fn test_parse_validator_info_active_ongoing() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "123456".to_string(),
			status: "active_ongoing".to_string(),
			validator: ValidatorDetails {
				pubkey: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok(), "Should successfully parse active_ongoing validator");

		let info = result.unwrap();
		assert!(info.is_active, "Validator should be active");
		assert!(!info.is_slashed, "Validator should not be slashed");
		assert_eq!(info.validator_index, 123456);
	}

	#[test]
	fn test_parse_validator_info_active_exiting() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "789".to_string(),
			status: "active_exiting".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());

		let info = result.unwrap();
		assert!(info.is_active, "Validator with active_exiting should be considered active");
		assert!(!info.is_slashed);
		assert_eq!(info.validator_index, 789);
	}

	#[test]
	fn test_parse_validator_info_active_slashed() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "999".to_string(),
			status: "active_slashed".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: true,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());

		let info = result.unwrap();
		assert!(info.is_active, "Validator with active_slashed should be considered active");
		assert!(info.is_slashed, "Validator should be marked as slashed");
		assert_eq!(info.validator_index, 999);
	}

	#[test]
	fn test_parse_validator_info_inactive_pending() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "1000".to_string(),
			status: "pending_initialized".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());

		let info = result.unwrap();
		assert!(!info.is_active, "Pending validator should not be active");
		assert!(!info.is_slashed);
		assert_eq!(info.validator_index, 1000);
	}

	#[test]
	fn test_parse_validator_info_inactive_exited() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "2000".to_string(),
			status: "exited_unslashed".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());

		let info = result.unwrap();
		assert!(!info.is_active, "Exited validator should not be active");
		assert!(!info.is_slashed);
		assert_eq!(info.validator_index, 2000);
	}

	#[test]
	fn test_parse_validator_info_exited_slashed() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "3000".to_string(),
			status: "exited_slashed".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: true,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());

		let info = result.unwrap();
		assert!(!info.is_active, "Exited slashed validator should not be active");
		assert!(info.is_slashed, "Validator should be marked as slashed");
		assert_eq!(info.validator_index, 3000);
	}

	#[test]
	fn test_parse_validator_info_withdrawal_possible() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "4000".to_string(),
			status: "withdrawal_possible".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());

		let info = result.unwrap();
		assert!(!info.is_active, "Validator in withdrawal_possible should not be active");
		assert!(!info.is_slashed);
		assert_eq!(info.validator_index, 4000);
	}

	#[test]
	fn test_parse_validator_info_invalid_index() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "not_a_number".to_string(),
			status: "active_ongoing".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_err(), "Should fail to parse invalid validator index");

		let error_msg = format!("{}", result.unwrap_err());
		assert!(error_msg.contains("Failed to parse validator index"), "Error should mention validator index parsing");
	}

	#[test]
	fn test_parse_validator_info_zero_index() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "0".to_string(),
			status: "active_ongoing".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok(), "Should accept validator index 0");

		let info = result.unwrap();
		assert_eq!(info.validator_index, 0);
	}

	#[test]
	fn test_parse_validator_info_large_index() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "18446744073709551615".to_string(), // u64::MAX
			status: "active_ongoing".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok(), "Should accept max u64 validator index");

		let info = result.unwrap();
		assert_eq!(info.validator_index, u64::MAX);
	}

	#[test]
	fn test_parse_validator_info_negative_index() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "-1".to_string(),
			status: "active_ongoing".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_err(), "Should reject negative validator index");
	}

	// ========================================
	// Tests for get_proposer_for_slot
	// ========================================
	// Note: These are integration-style tests that require network access
	// In production, we would use a mock HTTP server (e.g., wiremock)

	#[tokio::test]
	#[ignore = "Requires network access to beacon API"]
	async fn test_get_proposer_for_slot_found() {
		let config = create_test_config();
		let client = BeaconApiClient::new(config).unwrap();

		// Use a recent slot number
		let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
		let genesis_time = 1606824023u64; // Ethereum mainnet genesis
		let current_slot = (current_time - genesis_time) / 12;

		// This test requires actual network connectivity
		match client.get_proposer_for_slot(current_slot).await {
			Ok(Some(duty)) => {
				// Verify the duty is for the correct slot
				let duty_slot = duty.parse_slot().unwrap();
				assert_eq!(duty_slot, current_slot);
			}
			Ok(None) => {
				// This is also valid - there might not be a duty for this specific slot
				println!("No duty found for slot {}", current_slot);
			}
			Err(e) => {
				println!("Network error (expected in test environment): {}", e);
			}
		}
	}

	#[tokio::test]
	#[ignore = "Requires network access to beacon API"]
	async fn test_get_proposer_for_slot_not_found() {
		let config = create_test_config();
		let client = BeaconApiClient::new(config).unwrap();

		// Use a future slot that's very unlikely to have a proposer duty yet
		let far_future_slot = 99999999999u64;

		match client.get_proposer_for_slot(far_future_slot).await {
			Ok(result) => {
				// Should either be None or fail to fetch
				println!("Result for far future slot: {:?}", result.is_some());
			}
			Err(e) => {
				// Network errors are expected in test environments
				println!("Network error (expected): {}", e);
			}
		}
	}

	#[test]
	fn test_epoch_to_slot_conversions() {
		// Test the epoch-to-slot conversion logic used by get_proposer_for_slot

		// Slot 0-31 should be in epoch 0
		assert_eq!(BeaconTiming::slot_to_epoch(0), 0);
		assert_eq!(BeaconTiming::slot_to_epoch(31), 0);

		// Slot 32-63 should be in epoch 1
		assert_eq!(BeaconTiming::slot_to_epoch(32), 1);
		assert_eq!(BeaconTiming::slot_to_epoch(63), 1);

		// Slot 64 should be in epoch 2
		assert_eq!(BeaconTiming::slot_to_epoch(64), 2);

		// Test large slot numbers
		let large_slot = 10000000u64;
		let epoch = BeaconTiming::slot_to_epoch(large_slot);
		assert_eq!(epoch, large_slot / 32);
	}

	#[test]
	fn test_validator_duty_slot_parsing() {
		use crate::types::beacon::ValidatorDuty;

		// Test valid slot parsing
		let duty = ValidatorDuty {
			validator_index: "123".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "12345".to_string(),
		};

		let parsed_slot = duty.parse_slot();
		assert!(parsed_slot.is_ok());
		assert_eq!(parsed_slot.unwrap(), 12345);

		// Test invalid slot parsing
		let invalid_duty = ValidatorDuty {
			validator_index: "123".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "not_a_number".to_string(),
		};

		let result = invalid_duty.parse_slot();
		assert!(result.is_err(), "Should fail to parse invalid slot number");
	}

	// ========================================
	// Tests for URL building and request handling
	// ========================================

	#[test]
	fn test_url_building_with_trailing_slash() {
		// Test that URL building handles both with and without trailing slashes correctly
		let base_with_slash = "https://example.com/";
		let base_without_slash = "https://example.com";
		let endpoint = "eth/v1/test";

		// Both should produce the same result
		let url1 = if base_with_slash.ends_with('/') {
			format!("{}{}", base_with_slash, endpoint)
		} else {
			format!("{}/{}", base_with_slash, endpoint)
		};

		let url2 = if base_without_slash.ends_with('/') {
			format!("{}{}", base_without_slash, endpoint)
		} else {
			format!("{}/{}", base_without_slash, endpoint)
		};

		assert_eq!(url1, "https://example.com/eth/v1/test");
		assert_eq!(url2, "https://example.com/eth/v1/test");
	}

	#[test]
	fn test_proposer_duties_endpoint_formatting() {
		// Test that the endpoint is correctly formatted for different epochs
		let epoch = 12345u64;
		let endpoint = format!("eth/v1/validator/duties/proposer/{}", epoch);
		assert_eq!(endpoint, "eth/v1/validator/duties/proposer/12345");

		// Test with epoch 0
		let epoch_zero = 0u64;
		let endpoint_zero = format!("eth/v1/validator/duties/proposer/{}", epoch_zero);
		assert_eq!(endpoint_zero, "eth/v1/validator/duties/proposer/0");
	}

	#[test]
	fn test_validator_status_endpoint_formatting() {
		use crate::types::delegation::BlsPublicKey;

		// Create a test BLS public key
		let pubkey = BlsPublicKey([0x42; 48]);
		let pubkey_hex = format!("0x{}", hex::encode(pubkey.0));

		let endpoint = format!("eth/v1/beacon/states/head/validators/{}", pubkey_hex);
		assert!(endpoint.starts_with("eth/v1/beacon/states/head/validators/0x"));
		assert!(endpoint.contains("42424242")); // Should contain the hex representation
	}

	// ========================================
	// Edge case tests
	// ========================================

	#[test]
	fn test_client_debug_impl() {
		// Verify that BeaconApiClient implements Debug properly
		let config = create_test_config();
		let client = BeaconApiClient::new(config).unwrap();

		let debug_str = format!("{:?}", client);
		assert!(debug_str.contains("BeaconApiClient"));
	}

	#[test]
	fn test_config_with_multiple_fallbacks() {
		let config = BeaconApiConfig {
			primary_endpoint: "https://primary.test".to_string(),
			fallback_endpoints: vec![
				"https://fallback1.test".to_string(),
				"https://fallback2.test".to_string(),
				"https://fallback3.test".to_string(),
				"https://fallback4.test".to_string(),
				"https://fallback5.test".to_string(),
			],
			request_timeout_secs: 5,
			genesis_time: 1606824023,
		};

		let client = BeaconApiClient::new(config).unwrap();
		assert_eq!(client.config.fallback_endpoints.len(), 5);
	}

	#[test]
	fn test_config_with_large_timeout() {
		let config = BeaconApiConfig {
			primary_endpoint: "https://test.example.com".to_string(),
			fallback_endpoints: vec![],
			request_timeout_secs: 300, // 5 minutes
			genesis_time: 1606824023,
		};

		let client = BeaconApiClient::new(config);
		assert!(client.is_ok());
		assert_eq!(client.unwrap().config.request_timeout_secs, 300);
	}

	#[test]
	fn test_genesis_time_variations() {
		// Test with different genesis times
		let configs = vec![
			1606824023u64, // Ethereum mainnet
			1695902400u64, // Holesky testnet
			0u64,          // Edge case: genesis at Unix epoch
		];

		for genesis in configs {
			let config = BeaconApiConfig {
				primary_endpoint: "https://test.example.com".to_string(),
				fallback_endpoints: vec![],
				request_timeout_secs: 10,
				genesis_time: genesis,
			};

			let client = BeaconApiClient::new(config);
			assert!(client.is_ok(), "Should accept genesis time {}", genesis);
		}
	}

	// ========================================
	// Additional edge case and error handling tests
	// ========================================

	#[test]
	fn test_validator_duty_pubkey_parsing_with_prefix() {
		use crate::types::beacon::ValidatorDuty;

		let duty = ValidatorDuty {
			validator_index: "100".to_string(),
			pubkey: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
			slot: "200".to_string(),
		};

		let result = duty.parse_pubkey();
		assert!(result.is_ok(), "Should parse pubkey with 0x prefix");
		assert_eq!(result.unwrap().0.len(), 48);
	}

	#[test]
	fn test_validator_duty_pubkey_parsing_without_prefix() {
		use crate::types::beacon::ValidatorDuty;

		let duty = ValidatorDuty {
			validator_index: "100".to_string(),
			pubkey: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
			slot: "200".to_string(),
		};

		let result = duty.parse_pubkey();
		assert!(result.is_ok(), "Should parse pubkey without 0x prefix");
		assert_eq!(result.unwrap().0.len(), 48);
	}

	#[test]
	fn test_validator_duty_pubkey_parsing_invalid_length() {
		use crate::types::beacon::ValidatorDuty;

		// Too short pubkey
		let duty = ValidatorDuty {
			validator_index: "100".to_string(),
			pubkey: "0x1234".to_string(),
			slot: "200".to_string(),
		};

		let result = duty.parse_pubkey();
		assert!(result.is_err(), "Should reject pubkey with invalid length");
	}

	#[test]
	fn test_validator_duty_pubkey_parsing_invalid_hex() {
		use crate::types::beacon::ValidatorDuty;

		// Invalid hex characters
		let duty = ValidatorDuty {
			validator_index: "100".to_string(),
			pubkey: "0xZZZZ567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
			slot: "200".to_string(),
		};

		let result = duty.parse_pubkey();
		assert!(result.is_err(), "Should reject pubkey with invalid hex characters");
	}

	#[test]
	fn test_validator_duty_index_parsing() {
		use crate::types::beacon::ValidatorDuty;

		let duty = ValidatorDuty {
			validator_index: "987654".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "100".to_string(),
		};

		let result = duty.parse_validator_index();
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 987654);
	}

	#[test]
	fn test_validator_duty_index_parsing_invalid() {
		use crate::types::beacon::ValidatorDuty;

		let duty = ValidatorDuty {
			validator_index: "invalid_index".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "100".to_string(),
		};

		let result = duty.parse_validator_index();
		assert!(result.is_err(), "Should fail to parse invalid validator index");
	}

	#[test]
	fn test_validator_duty_index_parsing_zero() {
		use crate::types::beacon::ValidatorDuty;

		let duty = ValidatorDuty {
			validator_index: "0".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "100".to_string(),
		};

		let result = duty.parse_validator_index();
		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 0);
	}

	#[test]
	fn test_parse_validator_info_empty_status() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "100".to_string(),
			status: "".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());
		let info = result.unwrap();
		// Empty status should be treated as inactive
		assert!(!info.is_active);
	}

	#[test]
	fn test_parse_validator_info_unknown_status() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "100".to_string(),
			status: "unknown_status_type".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());
		let info = result.unwrap();
		// Unknown status should be treated as inactive
		assert!(!info.is_active);
	}

	#[test]
	fn test_parse_validator_info_case_sensitivity() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		// Test that status matching is case-sensitive (as per spec)
		let data = ValidatorData {
			index: "100".to_string(),
			status: "ACTIVE_ONGOING".to_string(), // Uppercase should not match
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());
		let info = result.unwrap();
		// Uppercase should not be recognized as active
		assert!(!info.is_active);
	}

	#[test]
	fn test_parse_validator_info_pending_queued() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "5000".to_string(),
			status: "pending_queued".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());
		let info = result.unwrap();
		assert!(!info.is_active);
		assert!(!info.is_slashed);
	}

	#[test]
	fn test_parse_validator_info_withdrawal_done() {
		use crate::types::beacon::{ValidatorData, ValidatorDetails};

		let data = ValidatorData {
			index: "6000".to_string(),
			status: "withdrawal_done".to_string(),
			validator: ValidatorDetails {
				pubkey: "0xabcd".to_string(),
				slashed: false,
			},
		};

		let result = BeaconApiClient::parse_validator_info(&data);
		assert!(result.is_ok());
		let info = result.unwrap();
		assert!(!info.is_active);
		assert!(!info.is_slashed);
	}

	#[test]
	fn test_proposer_duties_response_structure() {
		use crate::types::beacon::{ProposerDutiesResponse, ValidatorDuty};

		// Test that we can create and serialize/deserialize the response structure
		let response = ProposerDutiesResponse {
			execution_optimistic: false,
			finalized: true,
			data: vec![
				ValidatorDuty {
					validator_index: "1".to_string(),
					pubkey: "0xabcd".to_string(),
					slot: "100".to_string(),
				},
				ValidatorDuty {
					validator_index: "2".to_string(),
					pubkey: "0xdef0".to_string(),
					slot: "101".to_string(),
				},
			],
		};

		assert_eq!(response.data.len(), 2);
		assert!(!response.execution_optimistic);
		assert!(response.finalized);
	}

	#[test]
	fn test_empty_proposer_duties_response() {
		use crate::types::beacon::ProposerDutiesResponse;

		// Test handling of empty duties list
		let response = ProposerDutiesResponse {
			execution_optimistic: false,
			finalized: true,
			data: vec![],
		};

		assert_eq!(response.data.len(), 0);
	}

	#[test]
	fn test_client_creation_preserves_endpoint_urls() {
		// Verify that client creation preserves the exact endpoint URLs
		let primary = "https://primary.example.com/api/v1";
		let fallback1 = "https://fallback1.example.com";
		let fallback2 = "https://fallback2.example.com/custom/path";

		let config = BeaconApiConfig {
			primary_endpoint: primary.to_string(),
			fallback_endpoints: vec![fallback1.to_string(), fallback2.to_string()],
			request_timeout_secs: 10,
			genesis_time: 1606824023,
		};

		let client = BeaconApiClient::new(config).unwrap();

		assert_eq!(client.config.primary_endpoint, primary);
		assert_eq!(client.config.fallback_endpoints[0], fallback1);
		assert_eq!(client.config.fallback_endpoints[1], fallback2);
	}

	#[test]
	fn test_bls_pubkey_hex_encoding() {
		use crate::types::delegation::BlsPublicKey;

		// Test that BLS public key is correctly hex-encoded for API requests
		let pubkey = BlsPublicKey([0xAB; 48]);
		let hex_encoded = hex::encode(pubkey.0);

		assert_eq!(hex_encoded.len(), 96); // 48 bytes = 96 hex characters
		assert!(hex_encoded.chars().all(|c| c.is_ascii_hexdigit()));
		assert!(hex_encoded.to_lowercase().contains("ab"));
	}

	#[test]
	fn test_slot_number_edge_cases() {
		// Test slot number parsing with edge cases
		use crate::types::beacon::ValidatorDuty;

		// Test slot 0
		let duty_zero = ValidatorDuty {
			validator_index: "0".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "0".to_string(),
		};
		assert_eq!(duty_zero.parse_slot().unwrap(), 0);

		// Test very large slot number
		let duty_large = ValidatorDuty {
			validator_index: "0".to_string(),
			pubkey: "0xabcd".to_string(),
			slot: "18446744073709551615".to_string(), // u64::MAX
		};
		assert_eq!(duty_large.parse_slot().unwrap(), u64::MAX);
	}
}
