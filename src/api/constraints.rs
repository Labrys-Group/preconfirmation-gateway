//! Constraints API client for delegation retrieval and constraint submission
//!
//! This module provides integration with the Constraints Relay API for:
//! - Fetching SignedDelegation messages for upcoming slots
//! - Submitting SignedConstraints messages to builders
//! - Managing retry logic and error handling

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
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

/// Response from constraint submission
#[derive(Debug, Clone, Deserialize)]
pub struct ConstraintSubmissionResponse {
	/// Success status
	pub success: bool,
	/// Optional message from relay
	pub message: Option<String>,
	/// Submission ID for tracking
	pub submission_id: Option<String>,
}

/// Error response from constraints API
#[derive(Debug, Clone, Deserialize)]
pub struct ConstraintsApiError {
	pub error: String,
	pub code: Option<u32>,
	pub details: Option<String>,
}

impl ConstraintsApiClient {
	/// Create a new Constraints API client
	pub fn new(config: ConstraintsApiConfig) -> Result<Self> {
		let client = Client::builder()
			.timeout(Duration::from_secs(config.request_timeout_secs))
			.build()
			.context("Failed to create HTTP client")?;

		Ok(Self { client, config })
	}

	/// Retrieve delegations for a specific slot
	///
	/// This is called proactively to fetch delegation authority from proposers
	/// before commitment requests arrive for that slot.
	pub async fn get_delegations_for_slot(&self, slot: u64) -> Result<Vec<SignedDelegation>> {
		let endpoint = format!("constraints/v1/delegations/{}", slot);
		let url = self.build_url(&endpoint);

		debug!(slot = slot, url = %url, "Fetching delegations");

		let response = self.client
			.get(&url)
			.header("Content-Type", "application/json")
			.header("User-Agent", "preconfirmation-gateway/0.1.0")
			.send()
			.await
			.with_context(|| format!("Failed to fetch delegations for slot {}", slot))?;

		match response.status() {
			StatusCode::OK => {
				let delegations_response: DelegationsResponse = response
					.json()
					.await
					.context("Failed to parse delegations response")?;

				debug!(
					slot = slot,
					count = delegations_response.delegations.len(),
					"Retrieved delegations"
				);

				Ok(delegations_response.delegations)
			}
			StatusCode::NOT_FOUND => {
				// No delegations found for this slot - this is normal
				debug!(slot = slot, "No delegations found for slot");
				Ok(vec![])
			}
			status => {
				let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
				anyhow::bail!(
					"Failed to fetch delegations for slot {}: HTTP {} - {}",
					slot,
					status,
					error_text
				);
			}
		}
	}

	/// Retrieve delegations for multiple slots
	pub async fn get_delegations_for_slots(&self, slots: &[u64]) -> Result<Vec<SignedDelegation>> {
		let mut all_delegations = Vec::new();

		for &slot in slots {
			match self.get_delegations_for_slot(slot).await {
				Ok(mut delegations) => {
					all_delegations.append(&mut delegations);
				}
				Err(e) => {
					error!(
						slot = slot,
						error = %e,
						"Failed to retrieve delegations for slot"
					);
					// Continue with other slots even if one fails
				}
			}
		}

		Ok(all_delegations)
	}

	/// Submit signed constraints to the relay
	///
	/// This must be called within the 8-second deadline for the target slot.
	/// The relay will forward constraints to authorized builders.
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
		let submission_payload = serde_json::to_value(constraints)
			.context("Failed to serialize constraints")?;

		let mut attempt = 0;
		let mut last_error = None;

		// Retry logic for constraint submission
		while attempt < self.config.max_retries {
			attempt += 1;

			match self.client
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
							let result: ConstraintSubmissionResponse = response
								.json()
								.await
								.context("Failed to parse constraint submission response")?;

							debug!(
								slot = constraints.message.slot,
								success = result.success,
								submission_id = result.submission_id,
								attempt = attempt,
								"Constraints submitted successfully"
							);

							return Ok(result);
						}
						StatusCode::TOO_MANY_REQUESTS => {
							warn!(
								slot = constraints.message.slot,
								attempt = attempt,
								"Rate limited by constraints API, retrying"
							);

							// Wait before retry (exponential backoff)
							let delay = Duration::from_millis(100 * (1 << attempt));
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
							let error_text = response.text().await
								.unwrap_or_else(|_| "Unknown error".to_string());

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

							// Don't retry on client errors (4xx)
							if status.is_client_error() && status != StatusCode::TOO_MANY_REQUESTS {
								break;
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

					// Wait before retry
					let delay = Duration::from_millis(100 * (1 << attempt));
					tokio::time::sleep(delay).await;
					continue;
				}
			}
		}

		// All retries exhausted
		error!(
			slot = constraints.message.slot,
			attempts = attempt,
			"Failed to submit constraints after all retries"
		);

		Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown submission error")))
	}

	/// Health check to verify constraints API connectivity
	pub async fn health_check(&self) -> Result<()> {
		let endpoint = "health";
		let url = self.build_url(endpoint);

		debug!(url = %url, "Performing constraints API health check");

		match self.client
			.get(&url)
			.header("User-Agent", "preconfirmation-gateway/0.1.0")
			.send()
			.await
		{
			Ok(response) => {
				if response.status().is_success() {
					debug!("Constraints API health check passed");
					Ok(())
				} else {
					let status = response.status();
					let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
					anyhow::bail!("Constraints API unhealthy: HTTP {} - {}", status, error_text);
				}
			}
			Err(e) => {
				error!(error = %e, "Constraints API health check failed");
				Err(e.into())
			}
		}
	}

	/// Build full URL from endpoint
	fn build_url(&self, endpoint: &str) -> String {
		let base = &self.config.relay_endpoint;
		if base.ends_with('/') {
			format!("{}{}", base, endpoint)
		} else {
			format!("{}/{}", base, endpoint)
		}
	}

	/// Get authorized builders from configuration
	pub fn get_authorized_builders(&self) -> &[String] {
		&self.config.authorized_builders
	}

	/// Check if constraint submission is within timing window
	pub fn is_within_submission_window(&self, slot: u64, genesis_time: u64) -> bool {
		use crate::types::beacon::BeaconTiming;
		BeaconTiming::is_within_constraint_window(genesis_time, slot)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::ConstraintsApiConfig;

	fn create_test_config() -> ConstraintsApiConfig {
		ConstraintsApiConfig {
			relay_endpoint: "https://relay.example.com".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			authorized_builders: vec![
				"0x1234".to_string(),
				"0x5678".to_string(),
			],
		}
	}

	#[test]
	fn test_client_creation() {
		let config = create_test_config();
		let client = ConstraintsApiClient::new(config);
		assert!(client.is_ok());
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
	fn test_authorized_builders() {
		let config = create_test_config();
		let client = ConstraintsApiClient::new(config).unwrap();

		let builders = client.get_authorized_builders();
		assert_eq!(builders.len(), 2);
		assert_eq!(builders[0], "0x1234");
		assert_eq!(builders[1], "0x5678");
	}

	// Integration tests would require actual relay endpoints
	// These should be marked with #[ignore] or put behind feature flags
}