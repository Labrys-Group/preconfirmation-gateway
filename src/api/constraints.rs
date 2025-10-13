//! Constraints API client for delegation retrieval and constraint submission
//!
//! This module provides integration with the Constraints Relay API for:
//! - Fetching SignedDelegation messages for upcoming slots
//! - Submitting SignedConstraints messages to builders
//! - Managing retry logic and error handling

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, error, warn};

use crate::config::ConstraintsApiConfig;
use crate::types::delegation::{SignedConstraints, SignedDelegation};

/// Constraints API client for relay operations
#[derive(Debug, Clone)]
pub struct ConstraintsApiClient {
	client: Client,
	config: ConstraintsApiConfig,
}

/// Response from delegations endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct DelegationsResponse {
	/// Array of signed delegation messages
	pub delegations: Vec<SignedDelegation>,
}

/// Response from constraint submission (spec says Empty on success)
#[derive(Debug, Clone)]
pub struct ConstraintSubmissionResponse {
	/// HTTP status code received
	pub status: u16,
}

/// Error response from constraints API
#[derive(Debug, Clone, Deserialize)]
pub struct ConstraintsApiError {
	pub error: String,
	pub code: Option<u32>,
}

impl ConstraintsApiClient {
	/// Creates a new ConstraintsApiClient configured from `config`.
	///
	/// The created client uses the request timeout specified by `config.request_timeout_secs`.
	/// Returns an error if the underlying HTTP client cannot be constructed or if the configuration
	/// is invalid (e.g., empty relay endpoint, zero timeout, or excessive retry count).
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The relay endpoint is empty
	/// - The request timeout is zero (would cause immediate timeouts)
	/// - The max_retries is unreasonably high (> 10)
	/// - The underlying HTTP client cannot be constructed
	///
	/// # Examples
	///
	pub fn new(config: ConstraintsApiConfig) -> Result<Self> {
		// Validate configuration
		if config.relay_endpoint.trim().is_empty() {
			anyhow::bail!("Relay endpoint cannot be empty");
		}

		if config.request_timeout_secs == 0 {
			anyhow::bail!("Request timeout must be greater than zero");
		}

		if config.max_retries > 10 {
			anyhow::bail!("Max retries cannot exceed 10 (got {})", config.max_retries);
		}

		let client = Client::builder()
			.timeout(Duration::from_secs(config.request_timeout_secs))
			.build()
			.context("Failed to create HTTP client")?;

		Ok(Self { client, config })
	}

	/// Fetches delegations for a given slot from the Constraints Relay API.
	///
	/// On success, returns the list of `SignedDelegation` messages associated with `slot`.
	/// If the API responds with 404 Not Found, this function returns an empty vector.
	/// For other HTTP error responses or request/parse failures, an error is returned.
	///
	/// # Examples
	///
	pub async fn get_delegations_for_slot(&self, slot: u64) -> Result<Vec<SignedDelegation>> {
		let endpoint = format!("constraints/v1/delegations/{}", slot);
		let url = self.build_url(&endpoint);

		debug!(slot = slot, url = %url, "Fetching delegations");

		let response = self
			.client
			.get(&url)
			.header("Content-Type", "application/json")
			.header("User-Agent", "preconfirmation-gateway/0.1.0")
			.send()
			.await
			.with_context(|| format!("Failed to fetch delegations for slot {}", slot))?;

		match response.status() {
			StatusCode::OK => {
				let delegations_response: DelegationsResponse =
					response.json().await.context("Failed to parse delegations response")?;

				debug!(slot = slot, count = delegations_response.delegations.len(), "Retrieved delegations");

				Ok(delegations_response.delegations)
			}
			StatusCode::NOT_FOUND => {
				// No delegations found for this slot - this is normal
				debug!(slot = slot, "No delegations found for slot");
				Ok(vec![])
			}
			status => {
				let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
				anyhow::bail!("Failed to fetch delegations for slot {}: HTTP {} - {}", slot, status, error_text);
			}
		}
	}

	/// Submit signed constraints to the relay for the target slot.
	///
	/// Attempts to POST the provided `SignedConstraints` to the relay's builder constraints endpoint,
	/// honoring the client's configured retry and backoff policy. On success returns the relay's
	/// `ConstraintSubmissionResponse`; on persistent failure returns the last observed error.
	///
	/// # Examples
	///
	pub async fn submit_constraints(&self, constraints: &SignedConstraints) -> Result<ConstraintSubmissionResponse> {
		let endpoint = "constraints/v0/builder/constraints";
		let url = self.build_url(endpoint);

		debug!(
			slot = constraints.message.slot,
			constraint_count = constraints.message.constraints.len(),
			url = %url,
			"Submitting constraints"
		);

		// Serialize constraints for submission
		let submission_payload = serde_json::to_value(constraints).context("Failed to serialize constraints")?;

		// Ensure at least one attempt is made, even when max_retries is 0
		let max_attempts = std::cmp::max(1, self.config.max_retries);
		let mut attempt = 0;
		let mut last_error = None;

		// Retry logic for constraint submission
		while attempt < max_attempts {
			attempt += 1;

			match self
				.client
				.post(&url)
				.header("Content-Type", "application/json")
				.header("User-Agent", "preconfirmation-gateway/0.1.0")
				.json(&submission_payload)
				.send()
				.await
			{
				Ok(response) => {
					match response.status() {
						StatusCode::OK | StatusCode::ACCEPTED => {
							let status_code = response.status().as_u16();

							debug!(
								slot = constraints.message.slot,
								status = status_code,
								attempt = attempt,
								"Constraints submitted successfully"
							);

							return Ok(ConstraintSubmissionResponse { status: status_code });
						}
						StatusCode::TOO_MANY_REQUESTS => {
							warn!(
								slot = constraints.message.slot,
								attempt = attempt,
								"Rate limited by constraints API, retrying"
							);

							// Wait before retry (exponential backoff with overflow protection)
							let shift = attempt.min(10); // Cap shift to prevent overflow
							let delay_ms = 100u64.saturating_mul(2u64.saturating_pow(shift as u32));
							let delay = Duration::from_millis(delay_ms.min(30_000)); // Max 30 seconds
							tokio::time::sleep(delay).await;

							last_error = Some(anyhow::anyhow!("Rate limited"));
							continue;
						}
						StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
							warn!(
								slot = constraints.message.slot,
								attempt = attempt,
								"Timeout from constraints API, retrying"
							);

							last_error = Some(anyhow::anyhow!("API timeout"));
							continue;
						}
						status => {
							let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());

							// Try to parse as API error
							if let Ok(api_error) = serde_json::from_str::<ConstraintsApiError>(&error_text) {
								last_error = Some(anyhow::anyhow!(
									"Constraints API error: {} (code: {:?})",
									api_error.error,
									api_error.code
								));
							} else {
								last_error = Some(anyhow::anyhow!(
									"Constraints submission failed: HTTP {} - {}",
									status,
									error_text
								));
							}

							// Don't retry on client errors (4xx) except TOO_MANY_REQUESTS
							if status.is_client_error() && status != StatusCode::TOO_MANY_REQUESTS {
								break;
							}

							// Apply exponential backoff for server errors (5xx) to avoid tight retry loops
							if status.is_server_error() {
								warn!(
									slot = constraints.message.slot,
									attempt = attempt,
									status = status.as_u16(),
									"Server error from constraints API, retrying with backoff"
								);

								// Wait before retry (exponential backoff with overflow protection)
								let shift = attempt.min(10); // Cap shift to prevent overflow
								let delay_ms = 100u64.saturating_mul(2u64.saturating_pow(shift as u32));
								let delay = Duration::from_millis(delay_ms.min(30_000)); // Max 30 seconds
								tokio::time::sleep(delay).await;
							}

							continue;
						}
					}
				}
				Err(e) => {
					warn!(
						slot = constraints.message.slot,
						attempt = attempt,
						error = %e,
						"HTTP error submitting constraints"
					);

					last_error = Some(e.into());

					// Wait before retry (exponential backoff with overflow protection)
					let shift = attempt.min(10); // Cap shift to prevent overflow
					let delay_ms = 100u64.saturating_mul(2u64.saturating_pow(shift as u32));
					let delay = Duration::from_millis(delay_ms.min(30_000)); // Max 30 seconds
					tokio::time::sleep(delay).await;
					continue;
				}
			}
		}

		// All retries exhausted
		error!(slot = constraints.message.slot, attempts = attempt, "Failed to submit constraints after all retries");

		Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown submission error")))
	}

	/// Appends the given API endpoint path to the client's configured relay endpoint, ensuring exactly one `/` separates them.
	///
	/// # Examples
	///
	fn build_url(&self, endpoint: &str) -> String {
		let base = &self.config.relay_endpoint;
		if base.ends_with('/') { format!("{}{}", base, endpoint) } else { format!("{}/{}", base, endpoint) }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::ConstraintsApiConfig;
	use crate::testing::mocks::create_test_bls_keypair;
	use crate::types::delegation::{BlsSignature, Constraint, ConstraintsMessage, SignedConstraints};
	use std::time::Duration;
	use tokio::time::timeout;

	/// Creates a sample `ConstraintsApiConfig` pre-filled with deterministic test values.
	///
	/// The configuration uses "https://relay.example.com" as the relay endpoint, a 10-second
	/// request timeout, a maximum of 3 retries, and two example authorized builder IDs.
	///
	/// # Examples
	///
	fn create_test_config() -> ConstraintsApiConfig {
		ConstraintsApiConfig {
			relay_endpoint: "https://relay.example.com".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			authorized_builders: vec!["0x1234".to_string(), "0x5678".to_string()],
		}
	}

	fn create_test_config_with_short_timeout() -> ConstraintsApiConfig {
		ConstraintsApiConfig {
			relay_endpoint: "https://invalid-endpoint.test".to_string(),
			request_timeout_secs: 1,
			max_retries: 1,
			authorized_builders: vec![],
		}
	}

	fn create_test_config_no_retries() -> ConstraintsApiConfig {
		ConstraintsApiConfig {
			relay_endpoint: "https://invalid-endpoint.test".to_string(),
			request_timeout_secs: 1,
			max_retries: 0,
			authorized_builders: vec![],
		}
	}

	fn create_test_signed_constraints() -> SignedConstraints {
		let (_proposer_sk, proposer_pk) = create_test_bls_keypair();
		let (_delegate_sk, delegate_pk) = create_test_bls_keypair();
		let slot = 12345u64;

		// Create a simple constraint
		let constraint = Constraint::from_inclusion_commitment(vec![1, 2, 3, 4]);

		let constraints_message = ConstraintsMessage::new(proposer_pk, delegate_pk, slot, vec![constraint], vec![]);

		SignedConstraints {
			message: constraints_message,
			signature: BlsSignature([42u8; 96]), // Mock signature
		}
	}

	#[test]
	fn test_client_creation() {
		let config = create_test_config();
		let client = ConstraintsApiClient::new(config);
		assert!(client.is_ok(), "Should be able to create constraints API client");

		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 10);
		assert_eq!(client.config.max_retries, 3);
	}

	#[test]
	fn test_client_creation_with_short_timeout() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config);
		assert!(client.is_ok(), "Should create client even with short timeout");

		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 1);
		assert_eq!(client.config.max_retries, 1);
	}

	#[test]
	fn test_client_creation_no_retries() {
		let config = create_test_config_no_retries();
		let client = ConstraintsApiClient::new(config);
		assert!(client.is_ok(), "Should create client even with no retries");

		let client = client.unwrap();
		assert_eq!(client.config.max_retries, 0);
	}

	#[test]
	fn test_url_building() {
		let config = create_test_config();
		let client = ConstraintsApiClient::new(config).unwrap();

		let url = client.build_url("test/endpoint");
		assert_eq!(url, "https://relay.example.com/test/endpoint");

		// Test with trailing slash
		let mut config_with_slash = create_test_config();
		config_with_slash.relay_endpoint = "https://relay.example.com/".to_string();
		let client_with_slash = ConstraintsApiClient::new(config_with_slash).unwrap();

		let url_with_slash = client_with_slash.build_url("test/endpoint");
		assert_eq!(url_with_slash, "https://relay.example.com/test/endpoint");
	}

	#[test]
	fn test_url_building_edge_cases() {
		let config = ConstraintsApiConfig {
			relay_endpoint: "https://example.com".to_string(),
			request_timeout_secs: 1,
			max_retries: 1,
			authorized_builders: vec![],
		};
		let client = ConstraintsApiClient::new(config).unwrap();

		// Test with empty endpoint
		let url = client.build_url("");
		assert_eq!(url, "https://example.com/");

		// Test with leading slash
		let url2 = client.build_url("/leading/slash");
		assert_eq!(url2, "https://example.com//leading/slash");

		// Test with multiple slashes
		let url3 = client.build_url("multiple//slashes");
		assert_eq!(url3, "https://example.com/multiple//slashes");
	}

	#[tokio::test]
	async fn test_get_delegations_for_slot_timeout() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config).unwrap();

		// This should fail due to invalid endpoint
		let result = timeout(Duration::from_secs(5), client.get_delegations_for_slot(12345)).await;

		assert!(result.is_ok(), "Request should complete within timeout");
		let delegations_result = result.unwrap();
		assert!(delegations_result.is_err(), "Should fail with invalid endpoint");
	}

	#[tokio::test]
	async fn test_get_delegations_error_handling() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config).unwrap();

		// Test with various slots
		for slot in [0, 12345, u64::MAX] {
			let result = client.get_delegations_for_slot(slot).await;
			assert!(result.is_err(), "Should fail due to invalid endpoint for slot {}", slot);

			// Verify error contains meaningful information
			let error = result.unwrap_err();
			let error_string = format!("{}", error);
			assert!(!error_string.is_empty(), "Error message should not be empty");
		}
	}

	#[tokio::test]
	async fn test_submit_constraints_timeout() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config).unwrap();
		let constraints = create_test_signed_constraints();

		// This should fail due to invalid endpoint
		let result = timeout(Duration::from_secs(10), client.submit_constraints(&constraints)).await;

		assert!(result.is_ok(), "Request should complete within timeout");
		let submission_result = result.unwrap();
		assert!(submission_result.is_err(), "Should fail with invalid endpoint");
	}

	#[tokio::test]
	async fn test_submit_constraints_no_retries() {
		let config = create_test_config_no_retries();
		let client = ConstraintsApiClient::new(config).unwrap();
		let constraints = create_test_signed_constraints();

		// Should fail immediately without retries
		let result = client.submit_constraints(&constraints).await;
		assert!(result.is_err(), "Should fail without retries");
	}

	#[tokio::test]
	async fn test_submit_constraints_error_handling() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config).unwrap();
		let constraints = create_test_signed_constraints();

		// Test that submission fails gracefully with meaningful error
		let result = client.submit_constraints(&constraints).await;
		assert!(result.is_err(), "Should fail due to invalid endpoint");

		let error = result.unwrap_err();
		let error_string = format!("{}", error);
		assert!(!error_string.is_empty(), "Error message should not be empty");
		assert!(
			error_string.contains("Failed to submit constraints") || error_string.contains("error"),
			"Error should mention submission failure or contain error details"
		);
	}

	#[tokio::test]
	async fn test_concurrent_delegations_requests() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config).unwrap();

		// Test multiple concurrent delegation requests
		let mut handles = Vec::new();

		for i in 0..5 {
			let client_clone = client.clone();
			let handle = tokio::spawn(async move { client_clone.get_delegations_for_slot(i + 100).await });
			handles.push(handle);
		}

		// Wait for all requests to complete
		for handle in handles {
			let result = handle.await.unwrap();
			// All should fail due to invalid endpoints, but shouldn't panic
			assert!(result.is_err(), "Concurrent delegation requests should handle errors gracefully");
		}
	}

	#[tokio::test]
	async fn test_concurrent_constraint_submissions() {
		let config = create_test_config_with_short_timeout();
		let client = ConstraintsApiClient::new(config).unwrap();

		// Test multiple concurrent constraint submissions
		let mut handles = Vec::new();

		for _i in 0..3 {
			// Fewer constraints as they're more expensive to create
			let client_clone = client.clone();
			let constraints = create_test_signed_constraints();
			let handle = tokio::spawn(async move { client_clone.submit_constraints(&constraints).await });
			handles.push(handle);
		}

		// Wait for all requests to complete
		for handle in handles {
			let result = handle.await.unwrap();
			// All should fail due to invalid endpoints, but shouldn't panic
			assert!(result.is_err(), "Concurrent constraint submissions should handle errors gracefully");
		}
	}

	#[test]
	fn test_client_clone() {
		let config = create_test_config();
		let client = ConstraintsApiClient::new(config).unwrap();

		// Test that client can be cloned
		let cloned_client = client.clone();

		// Verify the clone has the same configuration
		assert_eq!(client.config.relay_endpoint, cloned_client.config.relay_endpoint);
		assert_eq!(client.config.request_timeout_secs, cloned_client.config.request_timeout_secs);
		assert_eq!(client.config.max_retries, cloned_client.config.max_retries);
	}

	#[test]
	fn test_config_validation() {
		// Test that invalid configurations are properly rejected
		let mut config = create_test_config();

		// Test with empty relay endpoint
		config.relay_endpoint = "".to_string();
		let client = ConstraintsApiClient::new(config.clone());
		assert!(client.is_err(), "Should reject empty relay endpoint");
		let error_msg = format!("{}", client.unwrap_err());
		assert!(error_msg.contains("Relay endpoint cannot be empty"), "Error should mention empty endpoint");

		// Test with whitespace-only relay endpoint
		config.relay_endpoint = "   ".to_string();
		let client = ConstraintsApiClient::new(config.clone());
		assert!(client.is_err(), "Should reject whitespace-only relay endpoint");

		// Test with zero timeout
		config.relay_endpoint = "https://valid-endpoint.com".to_string();
		config.request_timeout_secs = 0;
		let client = ConstraintsApiClient::new(config.clone());
		assert!(client.is_err(), "Should reject zero timeout");
		let error_msg = format!("{}", client.unwrap_err());
		assert!(error_msg.contains("Request timeout must be greater than zero"), "Error should mention zero timeout");

		// Test with excessive retry count
		config.request_timeout_secs = 10;
		config.max_retries = 100;
		let client = ConstraintsApiClient::new(config);
		assert!(client.is_err(), "Should reject excessive retry count");
		let error_msg = format!("{}", client.unwrap_err());
		assert!(error_msg.contains("Max retries cannot exceed 10"), "Error should mention excessive retries");
	}

	#[test]
	fn test_client_creation_with_minimal_valid_config() {
		// Test that minimal valid configurations work
		let config = ConstraintsApiConfig {
			relay_endpoint: "https://minimal.example.com".to_string(),
			request_timeout_secs: 1,     // Minimal valid timeout
			max_retries: 0,              // Zero retries should be fine (no retries)
			authorized_builders: vec![], // Empty builders should be fine
		};

		let client = ConstraintsApiClient::new(config);
		assert!(client.is_ok(), "Should accept minimal valid configuration");

		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 1);
		assert_eq!(client.config.max_retries, 0);
		assert!(client.config.authorized_builders.is_empty());
	}

	#[test]
	fn test_client_creation_with_maximum_valid_config() {
		// Test that maximum valid configurations work
		let config = ConstraintsApiConfig {
			relay_endpoint: "https://maximal.example.com".to_string(),
			request_timeout_secs: 300, // 5 minutes should be fine
			max_retries: 10,           // Maximum allowed retries
			authorized_builders: vec!["0x1111".to_string(), "0x2222".to_string()],
		};

		let client = ConstraintsApiClient::new(config);
		assert!(client.is_ok(), "Should accept maximum valid configuration");

		let client = client.unwrap();
		assert_eq!(client.config.request_timeout_secs, 300);
		assert_eq!(client.config.max_retries, 10);
		assert_eq!(client.config.authorized_builders.len(), 2);
	}

	#[test]
	fn test_authorized_builders_config() {
		let config = ConstraintsApiConfig {
			relay_endpoint: "https://example.com".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			authorized_builders: vec!["0x1111".to_string(), "0x2222".to_string(), "0x3333".to_string()],
		};

		let client = ConstraintsApiClient::new(config).unwrap();

		// Verify authorized builders are preserved
		assert_eq!(client.config.authorized_builders.len(), 3);
		assert!(client.config.authorized_builders.contains(&"0x1111".to_string()));
		assert!(client.config.authorized_builders.contains(&"0x2222".to_string()));
		assert!(client.config.authorized_builders.contains(&"0x3333".to_string()));
	}

	#[test]
	fn test_constraints_serialization() {
		let constraints = create_test_signed_constraints();

		// Test that constraints can be serialized to JSON for submission
		let json_result = serde_json::to_value(&constraints);
		assert!(json_result.is_ok(), "Should be able to serialize constraints to JSON");

		let json_value = json_result.unwrap();
		assert!(json_value.is_object(), "Should serialize to JSON object");

		// Verify key fields are present
		assert!(json_value.get("message").is_some(), "Should have message field");
		assert!(json_value.get("signature").is_some(), "Should have signature field");
	}

	#[test]
	fn test_delegation_response_deserialization() {
		// Test that we can deserialize various delegation response formats
		let json_empty = r#"{"delegations": []}"#;
		let result: Result<DelegationsResponse, _> = serde_json::from_str(json_empty);
		assert!(result.is_ok(), "Should deserialize empty delegations response");

		let response = result.unwrap();
		assert_eq!(response.delegations.len(), 0);
	}

	#[test]
	fn test_constraint_submission_response_creation() {
		// Test response creation with various status codes
		let response_ok = ConstraintSubmissionResponse { status: 200 };
		assert_eq!(response_ok.status, 200);

		let response_accepted = ConstraintSubmissionResponse { status: 202 };
		assert_eq!(response_accepted.status, 202);
	}

	#[test]
	fn test_api_error_deserialization() {
		let json_error = r#"{"error": "Invalid request", "code": 400}"#;
		let result: Result<ConstraintsApiError, _> = serde_json::from_str(json_error);
		assert!(result.is_ok(), "Should deserialize API error");

		let error = result.unwrap();
		assert_eq!(error.error, "Invalid request");
		assert_eq!(error.code, Some(400));

		// Test error without code
		let json_error_no_code = r#"{"error": "Server error"}"#;
		let result: Result<ConstraintsApiError, _> = serde_json::from_str(json_error_no_code);
		assert!(result.is_ok(), "Should deserialize API error without code");

		let error = result.unwrap();
		assert_eq!(error.error, "Server error");
		assert_eq!(error.code, None);
	}

	// Integration tests would require actual relay endpoints
	// These should be marked with #[ignore] or put behind feature flags

	#[tokio::test]
	#[ignore = "Integration test - requires real constraints API"]
	async fn test_real_constraints_api_integration() {
		// This test would use real constraint endpoints and should only run in integration test mode
		let config = ConstraintsApiConfig {
			relay_endpoint: "https://relay.example.com".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			authorized_builders: vec![],
		};

		let client = ConstraintsApiClient::new(config).unwrap();

		// Test delegation fetch
		let result = client.get_delegations_for_slot(12345).await;
		match result {
			Ok(delegations) => println!("Got {} delegations", delegations.len()),
			Err(e) => println!("Integration test failed (expected in CI): {}", e),
		}

		// Test constraint submission (would need valid signed constraints)
		let constraints = create_test_signed_constraints();
		let result = client.submit_constraints(&constraints).await;
		match result {
			Ok(response) => println!("Submission success: status {}", response.status),
			Err(e) => println!("Submission failed (expected in CI): {}", e),
		}
	}
}
