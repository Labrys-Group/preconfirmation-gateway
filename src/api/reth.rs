use std::time::Duration;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, warn, error};

/// Reth RPC client for gas price oracle functionality
#[derive(Clone, Debug)]
pub struct RethApiClient {
    client: Client,
    endpoint: String,
    request_timeout: Duration,
}

/// Gas price information from Reth node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasPriceInfo {
    /// Current gas price in wei
    pub gas_price: u64,
    /// Latest block number
    pub block_number: u64,
    /// Timestamp when this data was retrieved
    pub timestamp: u64,
}

/// Fee history response from eth_feeHistory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeHistory {
    /// Block base fees per gas (including next block)
    #[serde(rename = "baseFeePerGas")]
    pub base_fee_per_gas: Vec<String>,
    /// Gas used ratios for each block
    #[serde(rename = "gasUsedRatio")]
    pub gas_used_ratio: Vec<f64>,
    /// Oldest block number in the range
    #[serde(rename = "oldestBlock")]
    pub oldest_block: String,
    /// Priority fee percentiles (optional)
    pub reward: Option<Vec<Vec<String>>>,
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
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8545".to_string(),
            request_timeout_secs: 10,
            max_retries: 3,
        }
    }
}

impl RethApiClient {
    /// Create a new Reth API client
    pub fn new(config: RethApiConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            endpoint: config.endpoint,
            request_timeout: Duration::from_secs(config.request_timeout_secs),
        })
    }

    /// Get current gas price using eth_gasPrice
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

        let gas_price = u64::from_str_radix(
            gas_price_hex.strip_prefix("0x").unwrap_or(gas_price_hex),
            16
        ).context("Failed to parse gas price hex value")?;

        // Get current block number for context
        let block_number = self.get_block_number().await.unwrap_or(0);

        let gas_price_info = GasPriceInfo {
            gas_price,
            block_number,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        debug!("Retrieved gas price: {} wei at block {}", gas_price, block_number);
        Ok(gas_price_info)
    }

    /// Get fee history using eth_feeHistory
    pub async fn get_fee_history(
        &self,
        block_count: u64,
        newest_block: &str,
        reward_percentiles: Option<&[f64]>,
    ) -> Result<FeeHistory> {
        debug!("Fetching fee history from Reth node: {} blocks from {}", block_count, newest_block);

        let percentiles = reward_percentiles.map(|p| p.iter().collect::<Vec<_>>());

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_feeHistory",
            "params": [
                format!("0x{:x}", block_count),
                newest_block,
                percentiles
            ],
            "id": 2
        });

        let response = self.make_rpc_call(payload).await
            .context("Failed to get fee history from Reth node")?;

        let fee_history: FeeHistory = serde_json::from_value(response["result"].clone())
            .context("Failed to parse fee history response")?;

        debug!("Retrieved fee history for {} blocks", fee_history.base_fee_per_gas.len());
        Ok(fee_history)
    }

    /// Get current block number
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

    /// Make a JSON-RPC call to the Reth node
    async fn make_rpc_call(&self, payload: Value) -> Result<Value> {
        let mut attempts = 0;
        let max_retries = 3;

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

    /// Test the connection to the Reth node
    pub async fn test_connection(&self) -> Result<()> {
        debug!("Testing connection to Reth node: {}", self.endpoint);

        let block_number = self.get_block_number().await
            .context("Failed to test Reth node connection")?;

        debug!("Successfully connected to Reth node, current block: {}", block_number);
        Ok(())
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
        // Test gas price hex parsing
        let gas_price = u64::from_str_radix("1dcd6500", 16).unwrap();
        assert_eq!(gas_price, 500000000); // 0.5 gwei

        let gas_price_with_prefix = u64::from_str_radix(
            "0x1dcd6500".strip_prefix("0x").unwrap(),
            16
        ).unwrap();
        assert_eq!(gas_price_with_prefix, 500000000);
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