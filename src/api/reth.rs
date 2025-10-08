use std::time::Duration;
use anyhow::{Context, Result};
use ethers_core::types::U256;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, warn};

/// Reth RPC client for gas price oracle functionality
#[derive(Clone, Debug)]
pub struct RethApiClient {
    client: Client,
    endpoint: String,
    max_retries: u32,
}

/// Gas price information from Reth node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasPriceInfo {
    /// Current gas price in wei (256-bit integer)
    #[serde(with = "u256_serde")]
    pub gas_price: U256,
    /// Latest block number
    pub block_number: u64,
    /// Timestamp when this data was retrieved
    pub timestamp: u64,
}

impl GasPriceInfo {
    /// Convert the stored gas price to a primitive integer, clamping to the maximum representable value on overflow.
    ///
    /// Returns the gas price as a `u64`; if the stored `U256` value is greater than `u64::MAX`, this returns `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ethers_core::types::U256;
    ///
    /// let info = crate::api::reth::GasPriceInfo { gas_price: U256::from(20u64), block_number: 0, timestamp: 0 };
    /// assert_eq!(info.gas_price_as_u64_clamped(), 20u64);
    ///
    /// let big_price = U256::from(u128::MAX);
    /// let big_info = crate::api::reth::GasPriceInfo { gas_price: big_price, block_number: 0, timestamp: 0 };
    /// assert_eq!(big_info.gas_price_as_u64_clamped(), u64::MAX);
    /// ```
    pub fn gas_price_as_u64_clamped(&self) -> u64 {
        if self.gas_price > U256::from(u64::MAX) {
            u64::MAX
        } else {
            self.gas_price.as_u64()
        }
    }

    /// Convert the stored `gas_price` to a `u64` if it does not overflow.
    ///
    /// Returns `Ok(u64)` containing the gas price when the value is less than or equal to `u64::MAX`,
    /// and an `Err` describing the overflow when the gas price is larger than `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ethers_core::types::U256;
    ///
    /// let info = crate::api::reth::GasPriceInfo { gas_price: U256::from(20u64), block_number: 0, timestamp: 0 };
    /// assert_eq!(info.gas_price_as_u64_checked().unwrap(), 20u64);
    /// ```
    pub fn gas_price_as_u64_checked(&self) -> Result<u64> {
        if self.gas_price > U256::from(u64::MAX) {
            Err(anyhow::anyhow!(
                "Gas price {} exceeds u64::MAX ({})",
                self.gas_price,
                u64::MAX
            ))
        } else {
            Ok(self.gas_price.as_u64())
        }
    }
}

/// Custom serde module for U256 serialization
mod u256_serde {
    use ethers_core::types::U256;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a `U256` into a hex string prefixed with `0x`.
    ///
    /// The value is formatted in lowercase hexadecimal without leading zeros (except zero itself)
    /// and emitted as a JSON string like `"0x1a2b3c"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ethers_core::types::U256;
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct Example {
    ///     #[serde(with = "crate::api::reth::u256_serde")]
    ///     value: U256,
    /// }
    ///
    /// let example = Example { value: U256::from(0x1a_u64) };
    /// let json = serde_json::to_string(&example).unwrap();
    /// assert_eq!(json, r#"{"value":"0x1a"}"#);
    /// ```
    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{:x}", value))
    }

    /// Deserialize a hex string (optionally prefixed with "0x") into a `U256`.
    ///
    /// Accepts a hex-like string, strips a leading `"0x"` if present, and parses the remainder as base-16.
    /// Returns a serde deserialization error if the string is not a valid hexadecimal representation for `U256`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ethers_core::types::U256;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Example {
    ///     #[serde(with = "crate::api::reth::u256_serde")]
    ///     value: U256,
    /// }
    ///
    /// let example: Example = serde_json::from_str(r#"{"value":"0x1dcd6500"}"#).unwrap();
    /// assert_eq!(example.value, U256::from(0x1dcd6500u64));
    /// ```
    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        U256::from_str_radix(s, 16).map_err(serde::de::Error::custom)
    }
}

/// Configuration for Reth API client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RethApiConfig {
    /// Primary Reth node endpoint
    pub endpoint: String,
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
    /// Maximum number of retries for failed requests
    pub max_retries: u32,
}

impl Default for RethApiConfig {
    /// Constructs a default RethApiConfig with a local RPC endpoint, a short request timeout, and a small retry count.
    ///
    /// # Examples
    ///
    /// ```
    /// let cfg = crate::api::reth::RethApiConfig::default();
    /// assert_eq!(cfg.endpoint, "http://localhost:8545");
    /// assert_eq!(cfg.request_timeout_secs, 10);
    /// assert_eq!(cfg.max_retries, 3);
    /// ```
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8545".to_string(),
            request_timeout_secs: 10,
            max_retries: 3,
        }
    }
}

impl RethApiClient {
    /// Constructs a new `RethApiClient` from the provided configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created with the configured timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// let cfg = RethApiConfig {
    ///     endpoint: "http://localhost:8545".to_string(),
    ///     request_timeout_secs: 5,
    ///     max_retries: 3,
    /// };
    /// let client = RethApiClient::new(cfg).expect("failed to create RethApiClient");
    /// ```
    pub fn new(config: RethApiConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            endpoint: config.endpoint,
            max_retries: config.max_retries,
        })
    }

    /// Fetches the current gas price from the configured Reth node and returns it with context.
    ///
    /// The returned `GasPriceInfo` contains the gas price as a `U256`, the block number at which
    /// the price was observed (or `0` if the block number could not be fetched), and the UNIX
    /// epoch timestamp (seconds) when the price was retrieved.
    ///
    /// # Returns
    ///
    /// `GasPriceInfo` containing the current gas price, the block number (or `0` if unavailable),
    /// and the retrieval timestamp (seconds since the UNIX epoch).
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use tokio::runtime::Runtime;
    /// # use crate::api::reth::{RethApiClient, RethApiConfig};
    /// let rt = Runtime::new().unwrap();
    /// let config = RethApiConfig { endpoint: "http://localhost:8545".into(), request_timeout_secs: 10, max_retries: 3 };
    /// let client = RethApiClient::new(config).unwrap();
    /// let info = rt.block_on(async { client.get_gas_price().await.unwrap() });
    /// // gas_price is a U256, block_number is a u64, timestamp is seconds since epoch
    /// assert!(info.timestamp > 0);
    /// ```
    pub async fn get_gas_price(&self) -> Result<GasPriceInfo> {
        debug!("Fetching gas price from Reth node: {}", self.endpoint);

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_gasPrice",
            "params": [],
            "id": 1
        });

        let response = self.make_rpc_call(payload).await
            .context("Failed to get gas price from Reth node")?;

        let gas_price_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid gas price response format"))?;

        let gas_price = U256::from_str_radix(
            gas_price_hex.strip_prefix("0x").unwrap_or(gas_price_hex),
            16
        ).context("Failed to parse gas price as U256 hex value")?;

        // Get current block number for context
        let block_number = match self.get_block_number().await {
            Ok(num) => num,
            Err(err) => {
                warn!("Failed to get block number: {}", err);
                0
            }
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let gas_price_info = GasPriceInfo {
            gas_price,
            block_number,
            timestamp,
        };

        debug!("Retrieved gas price: {} wei at block {}", gas_price, block_number);
        Ok(gas_price_info)
    }

    /// Retrieves the current block number from the configured Reth node.
    ///
    /// # Returns
    ///
    /// The current block number as a `u64`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(client: &crate::RethApiClient) -> anyhow::Result<()> {
    /// let block = client.get_block_number().await?;
    /// println!("block: {}", block);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_block_number(&self) -> Result<u64> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 3
        });

        let response = self.make_rpc_call(payload).await
            .context("Failed to get block number from Reth node")?;

        let block_hex = response["result"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid block number response format"))?;

        let block_number = u64::from_str_radix(
            block_hex.strip_prefix("0x").unwrap_or(block_hex),
            16
        ).context("Failed to parse block number hex value")?;

        Ok(block_number)
    }

    /// Perform a JSON-RPC POST to the configured Reth endpoint with retry logic.
    ///
    /// This method sends the provided JSON-RPC `payload` to the client's endpoint, retries on
    /// transient network or HTTP failures up to the client's configured `max_retries`, and
    /// treats a JSON `"error"` field in the RPC response as a failure.
    ///
    /// The `payload` should be a complete JSON-RPC request object (for example, produced by
    /// `serde_json::json!`).
    ///
    /// # Returns
    ///
    /// `Ok(Value)` with the parsed JSON-RPC response when a successful response without an `"error"`
    /// field is received, `Err` with context if all retries are exhausted or if the RPC response
    /// contains an `"error"` object.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use serde_json::json;
    ///
    /// // `client` is an existing `RethApiClient` instance.
    /// let payload = json!({
    ///     "jsonrpc": "2.0",
    ///     "method": "web3_clientVersion",
    ///     "params": [],
    ///     "id": 1
    /// });
    ///
    /// // Inside an async context:
    /// // let resp = client.make_rpc_call(payload).await?;
    /// ```
    async fn make_rpc_call(&self, payload: Value) -> Result<Value> {
        let mut attempts = 0;
        let max_retries = self.max_retries;

        while attempts < max_retries {
            match self.client
                .post(&self.endpoint)
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<Value>().await {
                            Ok(json_response) => {
                                if json_response.get("error").is_some() {
                                    return Err(anyhow::anyhow!(
                                        "RPC error: {}", json_response["error"]
                                    ));
                                }
                                return Ok(json_response);
                            }
                            Err(e) => {
                                warn!("Failed to parse response as JSON (attempt {}): {}", attempts + 1, e);
                            }
                        }
                    } else {
                        warn!("HTTP error from Reth node (attempt {}): {}", attempts + 1, response.status());
                    }
                }
                Err(e) => {
                    warn!("Network error connecting to Reth node (attempt {}): {}", attempts + 1, e);
                }
            }

            attempts += 1;
            if attempts < max_retries {
                tokio::time::sleep(Duration::from_millis(100 * attempts as u64)).await;
            }
        }

        Err(anyhow::anyhow!(
            "Failed to connect to Reth node after {} attempts", max_retries
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reth_client_creation() {
        let config = RethApiConfig::default();
        let client = RethApiClient::new(config);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_hex_parsing() {
        // Test gas price hex parsing with U256
        let gas_price = U256::from_str_radix("1dcd6500", 16).unwrap();
        assert_eq!(gas_price, U256::from(500000000u64)); // 0.5 gwei

        let gas_price_with_prefix = U256::from_str_radix(
            "0x1dcd6500".strip_prefix("0x").unwrap(),
            16
        ).unwrap();
        assert_eq!(gas_price_with_prefix, U256::from(500000000u64));

        // Test large values that exceed u64
        let large_price = U256::from_str_radix(
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            16
        ).unwrap();
        assert!(large_price > U256::from(u64::MAX));
    }

    #[tokio::test]
    async fn test_gas_price_conversion() {
        // Test normal gas price that fits in u64
        let normal_price = GasPriceInfo {
            gas_price: U256::from(20_000_000_000u64), // 20 gwei
            block_number: 100,
            timestamp: 1234567890,
        };
        assert_eq!(normal_price.gas_price_as_u64_clamped(), 20_000_000_000u64);
        assert_eq!(normal_price.gas_price_as_u64_checked().unwrap(), 20_000_000_000u64);

        // Test gas price that exceeds u64::MAX
        let huge_price = GasPriceInfo {
            gas_price: U256::from_str_radix(
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                16
            ).unwrap(),
            block_number: 100,
            timestamp: 1234567890,
        };
        // Should clamp to u64::MAX
        assert_eq!(huge_price.gas_price_as_u64_clamped(), u64::MAX);
        // Should return error
        assert!(huge_price.gas_price_as_u64_checked().is_err());
    }

    #[tokio::test]
    async fn test_config_serialization() {
        let config = RethApiConfig {
            endpoint: "http://localhost:8545".to_string(),
            request_timeout_secs: 30,
            max_retries: 5,
        };

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: RethApiConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(config.endpoint, deserialized.endpoint);
        assert_eq!(config.request_timeout_secs, deserialized.request_timeout_secs);
        assert_eq!(config.max_retries, deserialized.max_retries);
    }
}