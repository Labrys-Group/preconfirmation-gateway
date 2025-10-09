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

/// Response from constraint submission
#[derive(Debug, Clone, Deserialize)]
pub struct ConstraintSubmissionResponse {
	/// Success status
	pub success: bool,
	/// Submission ID for tracking
	pub submission_id: Option<String>,
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
	///
	/// # Errors
	///
	/// Returns an error if the underlying HTTP client cannot be constructed.
	///
	/// # Examples
	///
	/// ```ignore
	/// let cfg = ConstraintsApiConfig {
	///     request_timeout_secs: 5,
	///     relay_endpoint: "https://relay.example.com".to_string(),
	///     max_retries: 3,
	///     authorized_builders: vec![],
	/// };
	/// let client = ConstraintsApiClient::new(cfg).expect("client creation failed");
	/// ```ignore
	pub fn new(config: ConstraintsApiConfig) -> Result<Self> {
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
	/// ```ignoreno_run
	/// # async fn example_usage(client: &crate::api::constraints::ConstraintsApiClient) -> anyhow::Result<()> {
	/// let delegations = client.get_delegations_for_slot(12345).await?;
	/// println!("Got {} delegations", delegations.len());
	/// # Ok(()) }
	/// ```ignore
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
	/// ```ignoreno_run
	/// # use crate::api::constraints::{ConstraintsApiClient, ConstraintsApiConfig, SignedConstraints};
	/// # async fn _example() {
	/// // Construct a client and a SignedConstraints value appropriate for your environment,
	/// // then submit:
	/// // let client = ConstraintsApiClient::new(config).unwrap();
	/// // let constraints = SignedConstraints { /* ... */ };
	/// // let response = client.submit_constraints(&constraints).await.unwrap();
	/// # }
	/// ```ignore
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

		let mut attempt = 0;
		let mut last_error = None;

		// Retry logic for constraint submission
		while attempt < self.config.max_retries {
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
							let result: ConstraintSubmissionResponse =
								response.json().await.context("Failed to parse constraint submission response")?;

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
	/// ```ignoreno_run
	/// // When base has no trailing slash
	/// let url = client.build_url("test/endpoint");
	/// assert_eq!(url, "https://relay.example.com/test/endpoint");
	///
	/// // When base ends with a trailing slash
	/// let url2 = client.build_url("test/endpoint");
	/// assert_eq!(url2, "https://relay.example.com/test/endpoint");
	/// ```ignore
	fn build_url(&self, endpoint: &str) -> String {
		let base = &self.config.relay_endpoint;
		if base.ends_with('/') { format!("{}{}", base, endpoint) } else { format!("{}/{}", base, endpoint) }
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::ConstraintsApiConfig;

	/// Creates a sample `ConstraintsApiConfig` pre-filled with deterministic test values.
	///
	/// The configuration uses "https://relay.example.com" as the relay endpoint, a 10-second
	/// request timeout, a maximum of 3 retries, and two example authorized builder IDs.
	///
	/// # Examples
	///
	/// ```ignore
	/// let cfg = create_test_config();
	/// assert_eq!(cfg.relay_endpoint, "https://relay.example.com");
	/// assert_eq!(cfg.request_timeout_secs, 10);
	/// assert_eq!(cfg.max_retries, 3);
	/// assert!(cfg.authorized_builders.contains(&"0x1234".to_string()));
	/// ```ignore
	fn create_test_config() -> ConstraintsApiConfig {
		ConstraintsApiConfig {
			relay_endpoint: "https://relay.example.com".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			authorized_builders: vec!["0x1234".to_string(), "0x5678".to_string()],
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

	// Integration tests would require actual relay endpoints
	// These should be marked with #[ignore] or put behind feature flags
}
