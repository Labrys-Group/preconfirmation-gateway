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
	/// ```
	/// let cfg = ServerConfig::default();
	/// assert_eq!(cfg.host, "127.0.0.1");
	/// assert_eq!(cfg.port, 8080);
	/// ```
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
	/// ```
	/// let cfg = LoggingConfig::default();
	/// assert_eq!(cfg.level, "info");
	/// assert!(cfg.enable_method_tracing);
	/// assert!(cfg.traced_methods.contains(&"fee".to_string()));
	/// ```
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
	/// The default sets the `slasher_address` to the zero Ethereum address.
	///
	/// # Examples
	///
	/// ```
	/// let cfg = ValidationConfig::default();
	/// assert_eq!(cfg.slasher_address, "0x0000000000000000000000000000000000000000");
	/// ```
	fn default() -> Self {
		Self { slasher_address: "0x0000000000000000000000000000000000000000".to_string() }
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
	/// ```
	/// let cfg = BeaconApiConfig::default();
	/// assert!(cfg.primary_endpoint.contains("alchemy") || cfg.primary_endpoint.starts_with("http"));
	/// assert!(cfg.fallback_endpoints.is_empty());
	/// assert_eq!(cfg.request_timeout_secs, 30);
	/// assert_eq!(cfg.genesis_time, 1606824023);
	/// ```
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
	/// ```
	/// let cfg = crate::config::ConstraintsApiConfig::default();
	/// assert_eq!(cfg.relay_endpoint, "https://relay.example.com");
	/// assert_eq!(cfg.request_timeout_secs, 10);
	/// assert_eq!(cfg.max_retries, 3);
	/// assert!(cfg.authorized_builders.is_empty());
	/// ```
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
	/// ```
	/// let cfg = DelegationConfig::default();
	/// assert_eq!(cfg.lookahead_epochs, 2);
	/// assert_eq!(cfg.polling_interval_secs, 60);
	/// assert_eq!(cfg.cache_ttl_secs, 300);
	/// assert_eq!(cfg.domain_application_gateway, "0x00000002");
	/// ```
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
	/// ```
	/// let signing = SigningConfig::default();
	/// // committer_address is derived from the embedded ECDSA key (e.g., "0x...").
	/// assert!(signing.committer_address.starts_with("0x"));
	/// ```
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
	/// ```
	/// use std::env;
	/// // Set test keys (replace with valid test vectors in real tests)
	/// env::set_var("COMMITTER_PRIVATE_KEY", "0000000000000000000000000000000000000000000000000000000000000001");
	/// env::set_var("BLS_PRIVATE_KEY", "0101010101010101010101010101010101010101010101010101010101010101");
	/// let cfg = crate::config::SigningConfig::load().expect("failed to load signing config");
	/// assert!(!cfg.committer_address.is_empty());
	/// ```
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
	/// ```
	/// let hex = "0101010101010101010101010101010101010101010101010101010101010101";
	/// let result = parse_bls_key(hex);
	/// assert!(result.is_ok());
	/// let (sk, pk) = result.unwrap();
	/// // `sk` is a `BlsSecretKey` and `pk` is its corresponding `BlsPublicKey`.
	/// ```
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
	/// ```
	/// let cfg = Config::load().expect("failed to load config");
	/// // use cfg...
	/// ```
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

	/// Replace known endpoint placeholders with corresponding environment variables.
	///
	/// This updates `beacon_api.primary_endpoint`, `reth.endpoint`, and `constraints_api.relay_endpoint` when they contain
	/// `${BEACON_API_ENDPOINT}`, `${RETH_ENDPOINT}`, or `${CONSTRAINTS_API_ENDPOINT}` respectively; if the corresponding
	/// environment variable is not set the placeholder is left unchanged for later validation.
	///
	/// # Examples
	///
	/// ```
	/// use std::env;
	/// // construct a config with placeholders
	/// let mut cfg = crate::Config::default();
	/// cfg.beacon_api.primary_endpoint = "${BEACON_API_ENDPOINT}".to_string();
	/// cfg.reth.endpoint = "${RETH_ENDPOINT}".to_string();
	/// cfg.constraints_api.relay_endpoint = "${CONSTRAINTS_API_ENDPOINT}".to_string();
	///
	/// // set environment variables
	/// env::set_var("BEACON_API_ENDPOINT", "https://beacon.example");
	/// env::set_var("RETH_ENDPOINT", "http://reth.local:8545");
	/// env::set_var("CONSTRAINTS_API_ENDPOINT", "https://relay.example");
	///
	/// // perform substitution
	/// cfg.substitute_env_vars().unwrap();
	///
	/// assert_eq!(cfg.beacon_api.primary_endpoint, "https://beacon.example");
	/// assert_eq!(cfg.reth.endpoint, "http://reth.local:8545");
	/// assert_eq!(cfg.constraints_api.relay_endpoint, "https://relay.example");
	/// ```
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
	/// ```
	/// # use anyhow::Result;
	/// # fn try_validate() -> Result<()> {
	/// validate_beacon_endpoint("https://eth-mainnet.g.alchemy.com/v2/actual_key")?;
	/// # Ok(())
	/// # }
	/// # try_validate().unwrap();
	/// ```
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
	/// ```
	/// # use anyhow::Result;
	/// # fn try_main() -> Result<()> {
	/// let ok = super::validate_endpoint("https://api.example.com", "SVC_ENDPOINT", "MyService")?;
	/// assert!(ok.is_ok() == false || ok.is_ok()); // ensure type-checking; actual call above returns Ok(())
	/// assert!(super::validate_endpoint("http://localhost:8545", "RETH_ENDPOINT", "Reth").is_ok());
	/// assert!(super::validate_endpoint("${RETH_ENDPOINT}", "RETH_ENDPOINT", "Reth").is_err());
	/// # Ok(())
	/// # }
	/// ```
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
	/// ```
	/// use std::fs::File;
	/// use std::io::Write;
	/// use tempfile::tempdir;
	///
	/// // Create a temp directory and TOML config file
	/// let dir = tempdir().unwrap();
	/// let file_path = dir.path().join("config.toml");
	/// let mut file = File::create(&file_path).unwrap();
	/// writeln!(file, "database.url = \"postgresql://example/db\"").unwrap();
	///
	/// let cfg = crate::config::Config::load_from_file(&file_path).unwrap();
	/// assert_eq!(cfg.database.url, "postgresql://example/db");
	/// ```
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
	/// ```
	/// let cfg = Config::default();
	/// let url = cfg.database_url();
	/// assert_eq!(url, "postgresql://localhost/preconfirmation_gateway");
	/// ```
	pub fn database_url(&self) -> &str {
		&self.database.url
	}
}

impl Default for RethConfig {
	/// Create a RethConfig populated with sensible defaults.
	///
	/// # Examples
	///
	/// ```
	/// let cfg = RethConfig::default();
	/// assert_eq!(cfg.endpoint, "http://localhost:8545");
	/// assert_eq!(cfg.request_timeout_secs, 10);
	/// assert_eq!(cfg.max_retries, 3);
	/// ```
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
	/// ```
	/// let cfg = FeeConfig::default();
	/// assert_eq!(cfg.scaling_factor, 2.0);
	/// assert_eq!(cfg.default_gas_limit, 30_000_000);
	/// assert_eq!(cfg.min_fee_multiplier, 1.0);
	/// assert_eq!(cfg.max_fee_multiplier, 100.0);
	/// assert_eq!(cfg.cache_ttl_secs, 60);
	/// ```
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
