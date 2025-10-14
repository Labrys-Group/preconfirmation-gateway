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
	pub slasher_whitelist: Vec<String>,
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
#[derive(Clone)]
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

impl std::fmt::Debug for SigningConfig {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SigningConfig")
			.field("ecdsa_private_key", &"<redacted>")
			.field("bls_private_key", &"<redacted>")
			.field("bls_public_key", &"<redacted>")
			.field("committer_address", &self.committer_address)
			.finish()
	}
}

impl Default for ServerConfig {
	/// Creates a default ServerConfig with host "127.0.0.1" and port 8080.
	///
	/// # Examples
	///
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
	/// Default logging configuration with the "info" level and method tracing enabled.
	///
	/// The default includes traced methods: `"commitmentRequest"`, `"commitmentResult"`, `"slots"`, and `"fee"`.
	///
	/// # Examples
	///
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
	/// Creates a `ValidationConfig` populated with sensible defaults.
	///
	/// The default sets the `slasher_whitelist` to an empty list (no restrictions).
	///
	/// # Examples
	///
	fn default() -> Self {
		Self { slasher_whitelist: vec![] }
	}
}

impl Default for BeaconApiConfig {
	/// Creates a default BeaconApiConfig populated with sensible mainnet defaults.
	///
	/// The returned configuration targets Ethereum mainnet: it sets a commonly used
	/// provider placeholder for the primary endpoint, leaves fallback endpoints empty,
	/// uses a 30-second request timeout, and sets the known Ethereum mainnet genesis time.
	///
	/// # Examples
	///
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
	/// Creates a ConstraintsApiConfig populated with sensible defaults for local development and testing.
	///
	/// Defaults:
	/// - `relay_endpoint`: "https://relay.example.com"
	/// - `request_timeout_secs`: 10
	/// - `max_retries`: 3
	/// - `authorized_builders`: empty list
	///
	/// # Examples
	///
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
	/// Creates a DelegationConfig populated with sensible defaults.
	///
	/// The defaults are:
	/// - `lookahead_epochs = 2`
	/// - `polling_interval_secs = 60`
	/// - `cache_ttl_secs = 300`
	/// - `domain_application_gateway = "0x00000002"` (default domain separator; configure a production value)
	///
	/// # Examples
	///
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
	/// Creates a `SigningConfig` populated with deterministic default keys for local development.
	///
	/// The ECDSA private key is parsed from a fixed hex string and the BLS private key is created from
	/// 32 bytes of `0x01`; the BLS public key and the committer address are derived from those keys.
	/// These defaults are deterministic and insecure — do not use them in production.
	///
	/// # Examples
	///
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
	/// Load signing keys and derive the committer address from environment variables.
	///
	/// Expects the environment variables `COMMITTER_PRIVATE_KEY` (ECDSA private key hex) and
	/// `BLS_PRIVATE_KEY` (BLS private key hex) to be set. On success, returns a `SigningConfig`
	/// with parsed ECDSA and BLS key material and the derived committer address.
	///
	/// # Errors
	///
	/// Returns an error if either environment variable is missing or if a provided key is invalid.
	///
	/// # Examples
	///
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

	/// Parses a 32-byte BLS secret key from a hex string and returns the secret key together with its derived public key.
	///
	/// # Errors
	///
	/// Returns an error if the provided hex is not a valid 32-byte sequence or if the secret key cannot be constructed from the bytes.
	///
	/// # Examples
	///
	fn parse_bls_key(hex_str: &str) -> Result<(BlsSecretKey, BlsPublicKey)> {
		let key_bytes = crypto::parse_hex_bytes(hex_str, 32).context("Invalid BLS private key hex")?;

		let private_key =
			BlsSecretKey::from_bytes(&key_bytes).map_err(|e| anyhow::anyhow!("Invalid BLS private key: {:?}", e))?;

		let public_key = private_key.sk_to_pk();

		Ok((private_key, public_key))
	}
}

impl Config {
	/// Loads the application configuration by reading the TOML file, applying environment
	/// substitutions, loading signing keys from environment variables, and validating endpoints.
	///
	/// This performs the full configuration initialization flow:
	/// 1. Loads config values from "config.toml" (falls back to defaults if missing).
	/// 2. Replaces endpoint placeholders with environment variables when present.
	/// 3. Loads ECDSA/BLS signing keys from COMMITTER_PRIVATE_KEY and BLS_PRIVATE_KEY.
	/// 4. Validates beacon, Reth, and Constraints API endpoints.
	///
	/// # Returns
	///
	/// `Self` on success.
	///
	/// # Examples
	///
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

		// Validate slasher whitelist is not empty
		Self::validate_slasher_whitelist(&config.validation)?;

		Ok(config)
	}

	/// Replace known endpoint placeholders with corresponding environment variables.
	///
	/// This updates `beacon_api.primary_endpoint`, `reth.endpoint`, and `constraints_api.relay_endpoint` when they contain
	/// `${BEACON_API_ENDPOINT}`, `${RETH_ENDPOINT}`, or `${CONSTRAINTS_API_ENDPOINT}` respectively; if the corresponding
	/// environment variable is not set the placeholder is left unchanged for later validation.
	///
	/// # Examples
	///
	#[allow(clippy::collapsible_if)]
	fn substitute_env_vars(config: &mut Self) -> Result<()> {
		// Substitute in beacon API endpoint
		if config.beacon_api.primary_endpoint.contains("${BEACON_API_ENDPOINT}") {
			if let Ok(endpoint) = std::env::var("BEACON_API_ENDPOINT") {
				config.beacon_api.primary_endpoint = endpoint;
			}
		}
		// If env var not set, leave the placeholder for validation to catch

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

	/// Ensures the beacon API endpoint is configured and formatted correctly.
	///
	/// Errors if the endpoint is empty, contains a common placeholder value (for example `${BEACON_API_ENDPOINT}`, `YOUR_API_KEY`, `YOUR_PROJECT_ID`, or `REPLACE_ME`), or does not begin with `http://` or `https://`.
	///
	/// # Returns
	///
	/// `Ok(())` if the endpoint appears valid; otherwise an `Err` with a descriptive message.
	///
	/// # Examples
	///
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

	/// Validates a service endpoint string and returns an error if it is not a usable HTTP/HTTPS URL.
	///
	/// Checks that the endpoint is non-empty, does not contain common placeholder values (including
	/// the provided environment variable placeholder, `REPLACE_ME`, or `example.com`), and starts
	/// with `http://` or `https://`.
	///
	/// # Parameters
	///
	/// - `endpoint`: The endpoint URL to validate.
	/// - `env_var_name`: The environment variable name used in error messages and placeholder checks.
	/// - `service_name`: A human-friendly name for the service included in error messages.
	///
	/// # Returns
	///
	/// `Ok(())` if the endpoint appears valid, an `Err` with descriptive context otherwise.
	///
	/// # Examples
	///
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

	/// Validates that the slasher whitelist is not empty.
	///
	/// An empty whitelist would allow all slasher addresses, which is a security risk.
	/// The configuration must explicitly specify at least one allowed slasher contract address.
	///
	/// # Parameters
	///
	/// - `validation_config`: The validation configuration to check.
	///
	/// # Returns
	///
	/// `Ok(())` if the whitelist contains at least one address, an `Err` otherwise.
	///
	/// # Examples
	///
	fn validate_slasher_whitelist(validation_config: &ValidationConfig) -> Result<()> {
		if validation_config.slasher_whitelist.is_empty() {
			anyhow::bail!(
				"Slasher whitelist is empty. For security, you must configure at least one allowed slasher contract address in config.toml under [validation] slasher_whitelist"
			);
		}

		// Validate that all addresses in the whitelist are valid Ethereum addresses
		for address in &validation_config.slasher_whitelist {
			if !address.starts_with("0x") || address.len() != 42 {
				anyhow::bail!(
					"Invalid slasher address in whitelist: '{}'. Expected format: 0x followed by 40 hex characters (e.g., 0x1234567890123456789012345678901234567890)",
					address
				);
			}
		}

		Ok(())
	}

	/// Load configuration from a TOML file, falling back to defaults when the file is absent.
	///
	/// If the given path exists, the file is read and parsed as TOML into a `Config`. If the file
	/// does not exist, the default `Config` is returned and a warning is emitted.
	///
	/// # Returns
	///
	/// `Ok(Config)` loaded from the specified file or the default configuration when the file is
	/// missing; `Err` with context if reading or parsing the file fails.
	///
	/// # Examples
	///
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

	/// Get the configured database connection URL.
	///
	/// # Examples
	///
	pub fn database_url(&self) -> &str {
		&self.database.url
	}
}

impl Default for RethConfig {
	/// Create a RethConfig populated with sensible defaults.
	///
	/// # Examples
	///
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
	/// Creates a `FeeConfig` populated with conservative defaults for dynamic fee calculation.
	///
	/// The defaults are chosen to provide reasonable exponential scaling while bounding prices:
	/// - `scaling_factor = 2.0`
	/// - `default_gas_limit = 30_000_000`
	/// - `min_fee_multiplier = 1.0`
	/// - `max_fee_multiplier = 100.0`
	/// - `cache_ttl_secs = 60`
	///
	/// # Examples
	///
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

#[cfg(test)]
mod tests {
	use super::*;
	use serial_test::serial;
	use std::env;
	use std::fs;
	use tempfile::TempDir;

	/// Helper function to create a temporary config file
	fn create_temp_config_file(content: &str) -> TempDir {
		let temp_dir = TempDir::new().unwrap();
		let config_path = temp_dir.path().join("config.toml");
		fs::write(&config_path, content).unwrap();
		temp_dir
	}

	/// Helper function to create a test config with custom values
	fn create_test_config() -> Config {
		let database_url = std::env::var("TEST_DATABASE_URL")
			.context("Test database env is required")
			.expect("TEST_DATABASE_URL must be set for tests");
		Config {
			server: ServerConfig { host: "127.0.0.1".to_string(), port: 8080 },
			database: DatabaseConfig { url: database_url },
			logging: LoggingConfig {
				level: "info".to_string(),
				enable_method_tracing: true,
				traced_methods: vec!["test_method".to_string()],
			},
			validation: ValidationConfig {
				slasher_whitelist: vec!["0x1234567890123456789012345678901234567890".to_string()],
			},
			beacon_api: BeaconApiConfig {
				primary_endpoint: "https://test.beacon.com".to_string(),
				fallback_endpoints: vec!["https://fallback.beacon.com".to_string()],
				request_timeout_secs: 30,
				genesis_time: 1606824023,
			},
			constraints_api: ConstraintsApiConfig {
				relay_endpoint: "https://test.relay.com".to_string(),
				request_timeout_secs: 10,
				max_retries: 3,
				authorized_builders: vec!["builder1".to_string(), "builder2".to_string()],
			},
			delegation: DelegationConfig {
				lookahead_epochs: 2,
				polling_interval_secs: 60,
				cache_ttl_secs: 300,
				domain_application_gateway: "0x00000002".to_string(),
			},
			reth: RethConfig {
				endpoint: "http://localhost:8545".to_string(),
				request_timeout_secs: 10,
				max_retries: 3,
				fee_config: FeeConfig {
					scaling_factor: 2.0,
					default_gas_limit: 30_000_000,
					min_fee_multiplier: 1.0,
					max_fee_multiplier: 100.0,
					cache_ttl_secs: 60,
				},
			},
			signing: SigningConfig::default(),
		}
	}

	#[test]
	fn test_server_config_default() {
		let config = ServerConfig::default();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 8080);
	}

	#[test]
	fn test_database_config_default() {
		let config = DatabaseConfig::default();
		assert_eq!(config.url, "postgresql://localhost/preconfirmation_gateway");
	}

	#[test]
	fn test_logging_config_default() {
		let config = LoggingConfig::default();
		assert_eq!(config.level, "info");
		assert!(config.enable_method_tracing);
		assert!(config.traced_methods.contains(&"commitmentRequest".to_string()));
		assert!(config.traced_methods.contains(&"commitmentResult".to_string()));
		assert!(config.traced_methods.contains(&"slots".to_string()));
		assert!(config.traced_methods.contains(&"fee".to_string()));
	}

	#[test]
	fn test_validation_config_default() {
		let config = ValidationConfig::default();
		assert_eq!(config.slasher_whitelist, Vec::<String>::new());
	}

	#[test]
	fn test_beacon_api_config_default() {
		let config = BeaconApiConfig::default();
		assert!(config.primary_endpoint.contains("alchemy.com"));
		assert!(config.fallback_endpoints.is_empty());
		assert_eq!(config.request_timeout_secs, 30);
		assert_eq!(config.genesis_time, 1606824023);
	}

	#[test]
	fn test_constraints_api_config_default() {
		let config = ConstraintsApiConfig::default();
		assert_eq!(config.relay_endpoint, "https://relay.example.com");
		assert_eq!(config.request_timeout_secs, 10);
		assert_eq!(config.max_retries, 3);
		assert!(config.authorized_builders.is_empty());
	}

	#[test]
	fn test_delegation_config_default() {
		let config = DelegationConfig::default();
		assert_eq!(config.lookahead_epochs, 2);
		assert_eq!(config.polling_interval_secs, 60);
		assert_eq!(config.cache_ttl_secs, 300);
		assert_eq!(config.domain_application_gateway, "0x00000002");
	}

	#[test]
	fn test_reth_config_default() {
		let config = RethConfig::default();
		assert_eq!(config.endpoint, "http://localhost:8545");
		assert_eq!(config.request_timeout_secs, 10);
		assert_eq!(config.max_retries, 3);
	}

	#[test]
	fn test_fee_config_default() {
		let config = FeeConfig::default();
		assert_eq!(config.scaling_factor, 2.0);
		assert_eq!(config.default_gas_limit, 30_000_000);
		assert_eq!(config.min_fee_multiplier, 1.0);
		assert_eq!(config.max_fee_multiplier, 100.0);
		assert_eq!(config.cache_ttl_secs, 60);
	}

	#[test]
	fn test_signing_config_default() {
		let config = SigningConfig::default();
		// Test that the default config creates valid keys
		assert!(!config.committer_address.is_empty());
		assert!(config.committer_address.starts_with("0x"));
	}

	#[test]
	fn test_config_load_from_missing_file() {
		let result = Config::load_from_file("nonexistent_config.toml");
		assert!(result.is_ok());
		let config = result.unwrap();
		// Should return default config
		assert_eq!(config.server.host, "127.0.0.1");
	}

	#[test]
	fn test_config_load_from_invalid_toml() {
		let temp_dir = create_temp_config_file("invalid toml content");
		let config_path = temp_dir.path().join("config.toml");
		let result = Config::load_from_file(&config_path);
		assert!(result.is_err());
	}

	#[test]
	fn test_config_load_from_valid_toml() {
		let config_content = r#"
[server]
host = "0.0.0.0"
port = 9090

[database]
url = "postgresql://custom:custom@localhost/custom_db"

[logging]
level = "debug"
enable_method_tracing = false
traced_methods = ["custom_method"]

[validation]
slasher_whitelist = ["0x1234567890123456789012345678901234567890"]

[beacon_api]
primary_endpoint = "https://test.beacon.com"
fallback_endpoints = []
request_timeout_secs = 30
genesis_time = 1606824023

[constraints_api]
relay_endpoint = "https://test.relay.com"
request_timeout_secs = 10
max_retries = 3
authorized_builders = []

[delegation]
lookahead_epochs = 2
polling_interval_secs = 60
cache_ttl_secs = 300
domain_application_gateway = "0x00000002"

[reth]
endpoint = "http://localhost:8545"
request_timeout_secs = 10
max_retries = 3

[reth.fee_config]
scaling_factor = 2.0
default_gas_limit = 30000000
min_fee_multiplier = 1.0
max_fee_multiplier = 100.0
cache_ttl_secs = 60
"#;
		let temp_dir = create_temp_config_file(config_content);
		let config_path = temp_dir.path().join("config.toml");
		let result = Config::load_from_file(&config_path);
		if let Err(e) = &result {
			println!("Error: {}", e);
		}
		assert!(result.is_ok());

		let config = result.unwrap();
		assert_eq!(config.server.host, "0.0.0.0");
		assert_eq!(config.server.port, 9090);
		assert_eq!(config.database.url, "postgresql://custom:custom@localhost/custom_db");
		assert_eq!(config.logging.level, "debug");
		assert!(!config.logging.enable_method_tracing);
		assert_eq!(config.logging.traced_methods, vec!["custom_method"]);
	}

	#[test]
	#[serial]
	fn test_environment_variable_substitution() {
		// Save original environment
		let original_beacon = env::var("BEACON_API_ENDPOINT").ok();
		let original_reth = env::var("RETH_ENDPOINT").ok();
		let original_constraints = env::var("CONSTRAINTS_API_ENDPOINT").ok();

		// Set test environment variables
		unsafe {
			env::set_var("BEACON_API_ENDPOINT", "https://env.beacon.com");
			env::set_var("RETH_ENDPOINT", "https://env.reth.com");
			env::set_var("CONSTRAINTS_API_ENDPOINT", "https://env.constraints.com");
		}

		let mut config = create_test_config();
		config.beacon_api.primary_endpoint = "${BEACON_API_ENDPOINT}".to_string();
		config.reth.endpoint = "${RETH_ENDPOINT}".to_string();
		config.constraints_api.relay_endpoint = "${CONSTRAINTS_API_ENDPOINT}".to_string();

		let result = Config::substitute_env_vars(&mut config);
		assert!(result.is_ok());

		assert_eq!(config.beacon_api.primary_endpoint, "https://env.beacon.com");
		assert_eq!(config.reth.endpoint, "https://env.reth.com");
		assert_eq!(config.constraints_api.relay_endpoint, "https://env.constraints.com");

		// Restore original environment
		unsafe {
			match original_beacon {
				Some(val) => env::set_var("BEACON_API_ENDPOINT", val),
				None => env::remove_var("BEACON_API_ENDPOINT"),
			}
			match original_reth {
				Some(val) => env::set_var("RETH_ENDPOINT", val),
				None => env::remove_var("RETH_ENDPOINT"),
			}
			match original_constraints {
				Some(val) => env::set_var("CONSTRAINTS_API_ENDPOINT", val),
				None => env::remove_var("CONSTRAINTS_API_ENDPOINT"),
			}
		}
	}

	#[test]
	fn test_environment_variable_substitution_missing() {
		let mut config = create_test_config();
		config.beacon_api.primary_endpoint = "${MISSING_ENV_VAR}".to_string();

		let result = Config::substitute_env_vars(&mut config);
		assert!(result.is_ok());

		// Should remain unchanged when env var is missing
		assert_eq!(config.beacon_api.primary_endpoint, "${MISSING_ENV_VAR}");
	}

	#[test]
	fn test_validate_beacon_endpoint_empty() {
		let result = Config::validate_beacon_endpoint("");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("empty"));
	}

	#[test]
	fn test_validate_beacon_endpoint_placeholder() {
		let result = Config::validate_beacon_endpoint("${BEACON_API_ENDPOINT}");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("placeholder"));

		let result = Config::validate_beacon_endpoint("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("YOUR_API_KEY"));
	}

	#[test]
	fn test_validate_beacon_endpoint_invalid_url() {
		let result = Config::validate_beacon_endpoint("not-a-url");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("HTTP/HTTPS URL"));
	}

	#[test]
	fn test_validate_beacon_endpoint_valid() {
		let result = Config::validate_beacon_endpoint("https://eth-mainnet.g.alchemy.com/v2/actual_key");
		assert!(result.is_ok());

		let result = Config::validate_beacon_endpoint("http://localhost:8545");
		assert!(result.is_ok());
	}

	#[test]
	fn test_validate_endpoint_empty() {
		let result = Config::validate_endpoint("", "TEST_ENDPOINT", "Test Service");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("empty"));
	}

	#[test]
	fn test_validate_endpoint_placeholder() {
		let result = Config::validate_endpoint("${TEST_ENDPOINT}", "TEST_ENDPOINT", "Test Service");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("placeholder"));

		let result = Config::validate_endpoint("REPLACE_ME", "TEST_ENDPOINT", "Test Service");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("REPLACE_ME"));

		let result = Config::validate_endpoint("example.com", "TEST_ENDPOINT", "Test Service");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("example.com"));
	}

	#[test]
	fn test_validate_endpoint_invalid_url() {
		let result = Config::validate_endpoint("not-a-url", "TEST_ENDPOINT", "Test Service");
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("HTTP/HTTPS URL"));
	}

	#[test]
	fn test_validate_endpoint_valid() {
		let result = Config::validate_endpoint("https://api.testservice.com", "TEST_ENDPOINT", "Test Service");
		if let Err(e) = &result {
			println!("Error: {}", e);
		}
		assert!(result.is_ok());

		let result = Config::validate_endpoint("http://localhost:8080", "TEST_ENDPOINT", "Test Service");
		if let Err(e) = &result {
			println!("Error: {}", e);
		}
		assert!(result.is_ok());
	}

	#[test]
	#[serial]
	fn test_signing_config_load_missing_env_vars() {
		// Save original environment
		let original_committer = env::var("COMMITTER_PRIVATE_KEY").ok();
		let original_bls = env::var("BLS_PRIVATE_KEY").ok();

		// Remove environment variables
		unsafe {
			env::remove_var("COMMITTER_PRIVATE_KEY");
			env::remove_var("BLS_PRIVATE_KEY");
		}

		let result = SigningConfig::load();
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("COMMITTER_PRIVATE_KEY"));

		// Restore original environment
		unsafe {
			match original_committer {
				Some(val) => env::set_var("COMMITTER_PRIVATE_KEY", val),
				None => env::remove_var("COMMITTER_PRIVATE_KEY"),
			}
			match original_bls {
				Some(val) => env::set_var("BLS_PRIVATE_KEY", val),
				None => env::remove_var("BLS_PRIVATE_KEY"),
			}
		}
	}

	#[test]
	#[serial]
	fn test_signing_config_load_invalid_ecdsa_key() {
		// Save original environment
		let original_committer = env::var("COMMITTER_PRIVATE_KEY").ok();
		let original_bls = env::var("BLS_PRIVATE_KEY").ok();

		// Set invalid ECDSA key
		unsafe {
			env::set_var("COMMITTER_PRIVATE_KEY", "invalid_key");
			env::set_var("BLS_PRIVATE_KEY", "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef");
		}

		let result = SigningConfig::load();
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Invalid COMMITTER_PRIVATE_KEY"));

		// Restore original environment
		unsafe {
			match original_committer {
				Some(val) => env::set_var("COMMITTER_PRIVATE_KEY", val),
				None => env::remove_var("COMMITTER_PRIVATE_KEY"),
			}
			match original_bls {
				Some(val) => env::set_var("BLS_PRIVATE_KEY", val),
				None => env::remove_var("BLS_PRIVATE_KEY"),
			}
		}
	}

	#[test]
	#[serial]
	fn test_signing_config_load_invalid_bls_key() {
		// Save original environment
		let original_committer = env::var("COMMITTER_PRIVATE_KEY").ok();
		let original_bls = env::var("BLS_PRIVATE_KEY").ok();

		// Set invalid BLS key
		unsafe {
			env::set_var("COMMITTER_PRIVATE_KEY", "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef");
			env::set_var("BLS_PRIVATE_KEY", "invalid_bls_key");
		}

		let result = SigningConfig::load();
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Invalid BLS_PRIVATE_KEY"));

		// Restore original environment
		unsafe {
			match original_committer {
				Some(val) => env::set_var("COMMITTER_PRIVATE_KEY", val),
				None => env::remove_var("COMMITTER_PRIVATE_KEY"),
			}
			match original_bls {
				Some(val) => env::set_var("BLS_PRIVATE_KEY", val),
				None => env::remove_var("BLS_PRIVATE_KEY"),
			}
		}
	}

	#[test]
	fn test_signing_config_parse_bls_key_valid() {
		let valid_hex = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
		let result = SigningConfig::parse_bls_key(valid_hex);
		assert!(result.is_ok());
		let (private_key, public_key) = result.unwrap();
		// Test that we can derive public key from private key
		let derived_public = private_key.sk_to_pk();
		assert_eq!(public_key, derived_public);
	}

	#[test]
	fn test_signing_config_parse_bls_key_invalid_length() {
		let invalid_hex = "1234567890abcdef"; // Too short
		let result = SigningConfig::parse_bls_key(invalid_hex);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Invalid BLS private key hex"));
	}

	#[test]
	fn test_signing_config_parse_bls_key_invalid_hex() {
		let invalid_hex = "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg";
		let result = SigningConfig::parse_bls_key(invalid_hex);
		assert!(result.is_err());
	}

	#[test]
	fn test_signing_config_debug_formatting() {
		let config = SigningConfig::default();
		let debug_str = format!("{:?}", config);
		assert!(debug_str.contains("SigningConfig"));
		assert!(debug_str.contains("<redacted>"));
		assert!(debug_str.contains(&config.committer_address));
	}

	#[test]
	fn test_config_serialization() {
		let config = create_test_config();
		let serialized = toml::to_string(&config).unwrap();
		assert!(serialized.contains("host = \"127.0.0.1\""));
		assert!(serialized.contains("port = 8080"));
	}

	#[test]
	fn test_config_deserialization() {
		let config_content = r#"
[server]
host = "0.0.0.0"
port = 9090

[database]
url = "postgresql://test:test@localhost/test_db"

[logging]
level = "debug"
enable_method_tracing = false
traced_methods = []

[validation]
slasher_whitelist = ["0x1234567890123456789012345678901234567890"]

[beacon_api]
primary_endpoint = "https://test.beacon.com"
fallback_endpoints = ["https://fallback.beacon.com"]
request_timeout_secs = 30
genesis_time = 1606824023

[constraints_api]
relay_endpoint = "https://test.relay.com"
request_timeout_secs = 10
max_retries = 3
authorized_builders = ["builder1", "builder2"]

[delegation]
lookahead_epochs = 2
polling_interval_secs = 60
cache_ttl_secs = 300
domain_application_gateway = "0x00000002"

[reth]
endpoint = "http://localhost:8545"
request_timeout_secs = 10
max_retries = 3

[reth.fee_config]
scaling_factor = 2.0
default_gas_limit = 30000000
min_fee_multiplier = 1.0
max_fee_multiplier = 100.0
cache_ttl_secs = 60
"#;
		let config: Config = toml::from_str(config_content).unwrap();
		assert_eq!(config.server.host, "0.0.0.0");
		assert_eq!(config.server.port, 9090);
		assert_eq!(config.database.url, "postgresql://test:test@localhost/test_db");
		assert_eq!(config.logging.level, "debug");
		assert!(!config.logging.enable_method_tracing);
	}

	#[test]
	fn test_config_clone() {
		let config1 = create_test_config();
		let config2 = config1.clone();
		assert_eq!(config1.server.host, config2.server.host);
		assert_eq!(config1.server.port, config2.server.port);
		assert_eq!(config1.database.url, config2.database.url);
	}

	#[test]
	fn test_config_debug() {
		let config = create_test_config();
		let debug_str = format!("{:?}", config);
		assert!(debug_str.contains("Config"));
		assert!(debug_str.contains("127.0.0.1"));
		assert!(debug_str.contains("8080"));
	}

	#[test]
	fn test_fee_config_bounds() {
		let fee_config = FeeConfig::default();

		// Test that bounds are reasonable
		assert!(fee_config.min_fee_multiplier >= 1.0);
		assert!(fee_config.max_fee_multiplier > fee_config.min_fee_multiplier);
		assert!(fee_config.scaling_factor > 0.0);
		assert!(fee_config.default_gas_limit > 0);
		assert!(fee_config.cache_ttl_secs > 0);
	}

	#[test]
	fn test_beacon_api_fallback_endpoints() {
		let config = BeaconApiConfig {
			fallback_endpoints: vec![
				"https://fallback1.beacon.com".to_string(),
				"https://fallback2.beacon.com".to_string(),
			],
			..Default::default()
		};

		assert_eq!(config.fallback_endpoints.len(), 2);
		assert!(config.fallback_endpoints.contains(&"https://fallback1.beacon.com".to_string()));
		assert!(config.fallback_endpoints.contains(&"https://fallback2.beacon.com".to_string()));
	}

	#[test]
	fn test_constraints_api_authorized_builders() {
		let config = ConstraintsApiConfig {
			authorized_builders: vec!["builder1".to_string(), "builder2".to_string(), "builder3".to_string()],
			..Default::default()
		};

		assert_eq!(config.authorized_builders.len(), 3);
		assert!(config.authorized_builders.contains(&"builder1".to_string()));
		assert!(config.authorized_builders.contains(&"builder2".to_string()));
		assert!(config.authorized_builders.contains(&"builder3".to_string()));
	}

	#[test]
	fn test_delegation_config_domain_separator() {
		let config = DelegationConfig::default();
		assert_eq!(config.domain_application_gateway, "0x00000002");
		assert!(config.domain_application_gateway.starts_with("0x"));
		assert_eq!(config.domain_application_gateway.len(), 10); // 0x + 8 hex chars
	}

	#[test]
	fn test_reth_config_timeout_values() {
		let config = RethConfig::default();
		assert!(config.request_timeout_secs > 0);
		assert!(config.max_retries > 0);
		assert!(config.request_timeout_secs <= 300); // Reasonable upper bound
		assert!(config.max_retries <= 10); // Reasonable upper bound
	}

	#[test]
	fn test_logging_config_traced_methods() {
		let config = LoggingConfig::default();
		assert!(!config.traced_methods.is_empty());
		assert!(config.traced_methods.contains(&"commitmentRequest".to_string()));
		assert!(config.traced_methods.contains(&"commitmentResult".to_string()));
		assert!(config.traced_methods.contains(&"slots".to_string()));
		assert!(config.traced_methods.contains(&"fee".to_string()));
	}

	#[test]
	fn test_validation_config_slasher_whitelist() {
		let config = ValidationConfig::default();
		assert_eq!(config.slasher_whitelist, Vec::<String>::new());

		// Test with custom whitelist
		let config_with_whitelist =
			ValidationConfig { slasher_whitelist: vec!["0x1234567890123456789012345678901234567890".to_string()] };
		assert_eq!(config_with_whitelist.slasher_whitelist.len(), 1);
		assert!(config_with_whitelist.slasher_whitelist[0].starts_with("0x"));
		assert_eq!(config_with_whitelist.slasher_whitelist[0].len(), 42); // 0x + 40 hex chars
	}

	#[test]
	fn test_validate_slasher_whitelist_empty() {
		let config = ValidationConfig { slasher_whitelist: vec![] };
		let result = Config::validate_slasher_whitelist(&config);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Slasher whitelist is empty"));
	}

	#[test]
	fn test_validate_slasher_whitelist_valid() {
		let config = ValidationConfig {
			slasher_whitelist: vec![
				"0x1234567890123456789012345678901234567890".to_string(),
				"0xabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_string(),
			],
		};
		let result = Config::validate_slasher_whitelist(&config);
		assert!(result.is_ok());
	}

	#[test]
	fn test_validate_slasher_whitelist_invalid_format() {
		// Missing 0x prefix
		let config =
			ValidationConfig { slasher_whitelist: vec!["1234567890123456789012345678901234567890".to_string()] };
		let result = Config::validate_slasher_whitelist(&config);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Invalid slasher address"));

		// Too short
		let config = ValidationConfig { slasher_whitelist: vec!["0x1234567890".to_string()] };
		let result = Config::validate_slasher_whitelist(&config);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Invalid slasher address"));

		// Too long
		let config =
			ValidationConfig { slasher_whitelist: vec!["0x123456789012345678901234567890123456789012".to_string()] };
		let result = Config::validate_slasher_whitelist(&config);
		assert!(result.is_err());
		assert!(result.unwrap_err().to_string().contains("Invalid slasher address"));
	}

	#[test]
	fn test_beacon_api_genesis_time() {
		let config = BeaconApiConfig::default();
		assert_eq!(config.genesis_time, 1606824023); // Ethereum mainnet genesis
		assert!(config.genesis_time > 0);
	}

	#[test]
	fn test_config_load_full_flow() {
		// This test would require setting up environment variables
		// For now, we'll test the individual components
		let config_content = r#"
[server]
host = "0.0.0.0"
port = 8080

[database]
url = "postgresql://test:test@localhost/test_db"

[logging]
level = "info"
enable_method_tracing = true
traced_methods = []

[validation]
slasher_whitelist = ["0x0000000000000000000000000000000000000000"]

[beacon_api]
primary_endpoint = "https://eth-mainnet.g.alchemy.com/v2/test_key"
fallback_endpoints = []
request_timeout_secs = 30
genesis_time = 1606824023

[constraints_api]
relay_endpoint = "https://relay.example.com"
request_timeout_secs = 10
max_retries = 3
authorized_builders = []

[delegation]
lookahead_epochs = 2
polling_interval_secs = 60
cache_ttl_secs = 300
domain_application_gateway = "0x00000002"

[reth]
endpoint = "http://localhost:8545"
request_timeout_secs = 10
max_retries = 3

[reth.fee_config]
scaling_factor = 2.0
default_gas_limit = 30000000
min_fee_multiplier = 1.0
max_fee_multiplier = 100.0
cache_ttl_secs = 60
"#;
		let temp_dir = create_temp_config_file(config_content);
		let config_path = temp_dir.path().join("config.toml");

		let result = Config::load_from_file(&config_path);
		assert!(result.is_ok());

		let config = result.unwrap();
		assert_eq!(config.server.host, "0.0.0.0");
		assert_eq!(config.server.port, 8080);
		assert_eq!(config.database.url, "postgresql://test:test@localhost/test_db");
		assert_eq!(config.logging.level, "info");
		assert!(config.logging.enable_method_tracing);
		assert_eq!(config.validation.slasher_whitelist, vec!["0x0000000000000000000000000000000000000000".to_string()]);
		assert_eq!(config.beacon_api.primary_endpoint, "https://eth-mainnet.g.alchemy.com/v2/test_key");
		assert_eq!(config.constraints_api.relay_endpoint, "https://relay.example.com");
		assert_eq!(config.delegation.lookahead_epochs, 2);
		assert_eq!(config.reth.endpoint, "http://localhost:8545");
		assert_eq!(config.reth.fee_config.scaling_factor, 2.0);
	}
}
