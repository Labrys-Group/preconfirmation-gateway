use std::path::Path;

use anyhow::{Context, Result};
use blst::{min_pk::PublicKey as BlsPublicKey, min_pk::SecretKey as BlsSecretKey};
use secp256k1::SecretKey;
use serde::{Deserialize, Serialize};

use crate::crypto;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
	pub server: ServerConfig,
	pub database: DatabaseConfig,
	pub logging: LoggingConfig,
	pub validation: ValidationConfig,
	pub beacon_api: BeaconApiConfig,
	pub constraints_api: ConstraintsApiConfig,
	pub delegation: DelegationConfig,
	pub reth: RethConfig,
	#[serde(skip)]
	pub signing: SigningConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
	pub host: String,
	pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
	pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
	pub level: String,
	pub enable_method_tracing: bool,
	pub traced_methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
	pub slasher_address: String,
}

/// Configuration for Beacon API integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeaconApiConfig {
	/// Primary beacon node endpoint (e.g., Alchemy)
	pub primary_endpoint: String,
	/// Fallback beacon node endpoints
	pub fallback_endpoints: Vec<String>,
	/// Request timeout in seconds
	pub request_timeout_secs: u64,
	/// Beacon chain genesis time (Unix timestamp)
	pub genesis_time: u64,
}

/// Configuration for Constraints API integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintsApiConfig {
	/// Constraints relay endpoint
	pub relay_endpoint: String,
	/// Request timeout in seconds
	pub request_timeout_secs: u64,
	/// Maximum retries for failed requests
	pub max_retries: usize,
	/// Authorized builder public keys (empty = public access)
	pub authorized_builders: Vec<String>,
}

/// Configuration for delegation management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConfig {
	/// How far ahead to poll for delegations (in epochs)
	pub lookahead_epochs: u64,
	/// Polling interval in seconds
	pub polling_interval_secs: u64,
	/// How long to cache delegations in memory (seconds)
	pub cache_ttl_secs: u64,
	/// Configurable domain separator for constraint signing (hex string)
	pub domain_application_gateway: String,
}

/// Configuration for Reth node integration and fee oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RethConfig {
	/// Reth node RPC endpoint
	pub endpoint: String,
	/// Request timeout in seconds
	pub request_timeout_secs: u64,
	/// Maximum retries for failed requests
	pub max_retries: u32,
	/// Fee calculation parameters
	pub fee_config: FeeConfig,
}

/// Configuration for dynamic fee calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
	/// Exponential scaling factor (k) in pricing formula
	pub scaling_factor: f64,
	/// Default gas limit for calculations (30M gas)
	pub default_gas_limit: u64,
	/// Minimum fee multiplier (prevents zero fees)
	pub min_fee_multiplier: f64,
	/// Maximum fee multiplier (caps extreme pricing)
	pub max_fee_multiplier: f64,
	/// Fee cache TTL in seconds
	pub cache_ttl_secs: u64,
}

/// Signing configuration loaded from environment variables
/// This is kept separate from TOML config for security
#[derive(Debug, Clone)]
pub struct SigningConfig {
	/// ECDSA private key for commitment signing
	pub ecdsa_private_key: SecretKey,
	/// BLS private key for constraint signing
	pub bls_private_key: BlsSecretKey,
	/// Corresponding BLS public key
	pub bls_public_key: BlsPublicKey,
	/// Ethereum address derived from ECDSA key
	pub committer_address: String,
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self { host: "127.0.0.1".to_string(), port: 8080 }
	}
}

impl Default for DatabaseConfig {
	fn default() -> Self {
		Self { url: "postgresql://localhost/preconfirmation_gateway".to_string() }
	}
}

impl Default for LoggingConfig {
	fn default() -> Self {
		Self {
			level: "info".to_string(),
			enable_method_tracing: true,
			traced_methods: vec![
				"commitmentRequest".to_string(),
				"commitmentResult".to_string(),
				"slots".to_string(),
				"fee".to_string(),
			],
		}
	}
}

impl Default for ValidationConfig {
	fn default() -> Self {
		Self { slasher_address: "0x0000000000000000000000000000000000000000".to_string() }
	}
}

impl Default for BeaconApiConfig {
	fn default() -> Self {
		Self {
			primary_endpoint: "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY".to_string(),
			fallback_endpoints: vec![],
			request_timeout_secs: 30,
			// Ethereum mainnet genesis time
			genesis_time: 1606824023,
		}
	}
}

impl Default for ConstraintsApiConfig {
	fn default() -> Self {
		Self {
			relay_endpoint: "https://relay.example.com".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			authorized_builders: vec![],
		}
	}
}

impl Default for DelegationConfig {
	fn default() -> Self {
		Self {
			lookahead_epochs: 2,
			polling_interval_secs: 60,
			cache_ttl_secs: 300,
			// Default domain separator (should be configured in production)
			domain_application_gateway: "0x00000002".to_string(),
		}
	}
}

impl Default for SigningConfig {
	fn default() -> Self {
		let ecdsa_private_key =
			crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
				.expect("Failed to parse default private key");

		let bls_key_bytes = [0x01u8; 32];
		let bls_private_key =
			BlsSecretKey::from_bytes(&bls_key_bytes).expect("Failed to create default BLS private key");
		let bls_public_key = bls_private_key.sk_to_pk();

		let committer_address = crypto::ecdsa_to_address(&ecdsa_private_key).expect("Failed to derive default address");

		Self { ecdsa_private_key, bls_private_key, bls_public_key, committer_address }
	}
}

impl SigningConfig {
	/// Load signing configuration from environment variables
	/// Fails if required environment variables are not set
	pub fn load() -> Result<Self> {
		// Load ECDSA private key from COMMITTER_PRIVATE_KEY (required)
		let private_key_hex = std::env::var("COMMITTER_PRIVATE_KEY").context(
			"COMMITTER_PRIVATE_KEY environment variable is required. Set it to a valid ECDSA private key (64 hex characters)",
		)?;

		let ecdsa_private_key = crypto::parse_private_key(&private_key_hex)
			.context("Invalid COMMITTER_PRIVATE_KEY format. Expected 64 hex characters (32 bytes)")?;

		// Load BLS private key (required)
		let bls_hex = std::env::var("BLS_PRIVATE_KEY").context(
			"BLS_PRIVATE_KEY environment variable is required. Set it to a valid BLS private key (64 hex characters)",
		)?;

		let (bls_private_key, bls_public_key) = Self::parse_bls_key(&bls_hex)
			.context("Invalid BLS_PRIVATE_KEY format. Expected 64 hex characters (32 bytes)")?;

		let committer_address =
			crypto::ecdsa_to_address(&ecdsa_private_key).context("Failed to derive committer address")?;

		Ok(Self { ecdsa_private_key, bls_private_key, bls_public_key, committer_address })
	}

	/// Parse BLS private key and derive public key
	fn parse_bls_key(hex_str: &str) -> Result<(BlsSecretKey, BlsPublicKey)> {
		let key_bytes = crypto::parse_hex_bytes(hex_str, 32).context("Invalid BLS private key hex")?;

		let private_key =
			BlsSecretKey::from_bytes(&key_bytes).map_err(|e| anyhow::anyhow!("Invalid BLS private key: {:?}", e))?;

		let public_key = private_key.sk_to_pk();

		Ok((private_key, public_key))
	}
}

impl Config {
	pub fn load() -> Result<Self> {
		let mut config = Self::load_from_file("config.toml")?;

		// Substitute environment variables in configuration
		Self::substitute_env_vars(&mut config)?;

		// Load signing config from environment variables (fails if not provided)
		config.signing = SigningConfig::load()
			.context("Failed to load signing configuration from environment variables. Please set COMMITTER_PRIVATE_KEY and BLS_PRIVATE_KEY")?;

		// Validate all endpoints are properly configured
		Self::validate_beacon_endpoint(&config.beacon_api.primary_endpoint)?;
		Self::validate_endpoint(&config.reth.endpoint, "RETH_ENDPOINT", "Reth")?;
		Self::validate_endpoint(&config.constraints_api.relay_endpoint, "CONSTRAINTS_API_ENDPOINT", "Constraints API")?;

		Ok(config)
	}

	/// Substitute environment variables in configuration strings
	/// Replaces ${VAR_NAME} with the value of environment variable VAR_NAME
	fn substitute_env_vars(config: &mut Self) -> Result<()> {
		// Substitute in beacon API endpoint
		if config.beacon_api.primary_endpoint.contains("${BEACON_API_ENDPOINT}") {
			if let Ok(endpoint) = std::env::var("BEACON_API_ENDPOINT") {
				config.beacon_api.primary_endpoint = endpoint;
			}
			// If env var not set, leave the placeholder for validation to catch
		}

		// Substitute in reth endpoint
		if config.reth.endpoint.contains("${RETH_ENDPOINT}") {
			if let Ok(endpoint) = std::env::var("RETH_ENDPOINT") {
				config.reth.endpoint = endpoint;
			}
		}

		// Substitute in constraints API endpoint
		if config.constraints_api.relay_endpoint.contains("${CONSTRAINTS_API_ENDPOINT}") {
			if let Ok(endpoint) = std::env::var("CONSTRAINTS_API_ENDPOINT") {
				config.constraints_api.relay_endpoint = endpoint;
			}
		}

		Ok(())
	}

	/// Validate that the beacon API endpoint is properly configured
	fn validate_beacon_endpoint(endpoint: &str) -> Result<()> {
		if endpoint.is_empty() {
			anyhow::bail!(
				"Beacon API endpoint is empty. Please set BEACON_API_ENDPOINT environment variable or configure primary_endpoint in config.toml"
			);
		}

		// Check for common placeholder values that indicate the endpoint is not configured
		let placeholder_indicators = ["${BEACON_API_ENDPOINT}", "YOUR_API_KEY", "YOUR_PROJECT_ID", "REPLACE_ME"];

		for placeholder in &placeholder_indicators {
			if endpoint.contains(placeholder) {
				anyhow::bail!(
					"Beacon API endpoint contains placeholder '{}'. Please set BEACON_API_ENDPOINT environment variable with a valid endpoint. Example: https://eth-mainnet.g.alchemy.com/v2/YOUR_ACTUAL_KEY",
					placeholder
				);
			}
		}

		// Basic URL validation
		if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
			anyhow::bail!("Beacon API endpoint must be a valid HTTP/HTTPS URL, got: {}", endpoint);
		}

		Ok(())
	}

	/// Validate that an endpoint is properly configured (generic validator)
	fn validate_endpoint(endpoint: &str, env_var_name: &str, service_name: &str) -> Result<()> {
		if endpoint.is_empty() {
			anyhow::bail!(
				"{} endpoint is empty. Please set {} environment variable or configure the endpoint in config.toml",
				service_name,
				env_var_name
			);
		}

		// Check for common placeholder values that indicate the endpoint is not configured
		let placeholder_indicators =
			[format!("${{{}}}", env_var_name), "REPLACE_ME".to_string(), "example.com".to_string()];

		for placeholder in &placeholder_indicators {
			if endpoint.contains(placeholder) {
				anyhow::bail!(
					"{} endpoint contains placeholder '{}'. Please set {} environment variable with a valid endpoint",
					service_name,
					placeholder,
					env_var_name
				);
			}
		}

		// Basic URL validation
		if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
			anyhow::bail!("{} endpoint must be a valid HTTP/HTTPS URL, got: {}", service_name, endpoint);
		}

		Ok(())
	}

	pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
		let config_path = path.as_ref();

		if !config_path.exists() {
			tracing::warn!("Configuration file {} not found, using defaults", config_path.display());
			return Ok(Self::default());
		}

		let config_str = std::fs::read_to_string(config_path)
			.with_context(|| format!("Failed to read configuration file: {}", config_path.display()))?;

		let config: Config = toml::from_str(&config_str)
			.with_context(|| format!("Failed to parse configuration file: {}", config_path.display()))?;

		tracing::info!("Configuration loaded from {}", config_path.display());
		Ok(config)
	}

	pub fn database_url(&self) -> &str {
		&self.database.url
	}
}

impl Default for RethConfig {
	fn default() -> Self {
		Self {
			endpoint: "http://localhost:8545".to_string(),
			request_timeout_secs: 10,
			max_retries: 3,
			fee_config: FeeConfig::default(),
		}
	}
}

impl Default for FeeConfig {
	fn default() -> Self {
		Self {
			scaling_factor: 2.0,           // k=2 provides reasonable exponential scaling
			default_gas_limit: 30_000_000, // 30M gas typical block limit
			min_fee_multiplier: 1.0,       // Never less than base price
			max_fee_multiplier: 100.0,     // Cap at 100x base price
			cache_ttl_secs: 60,            // Cache fees for 1 minute
		}
	}
}
