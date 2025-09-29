use std::path::Path;

use anyhow::{Context, Result};
use blst::{min_pk::PublicKey as BlsPublicKey, min_pk::SecretKey as BlsSecretKey};
use serde::{Deserialize, Serialize};
use secp256k1::SecretKey;

use crate::crypto;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
	pub server: ServerConfig,
	pub database: DatabaseConfig,
	pub logging: LoggingConfig,
	pub validation: ValidationConfig,
	pub beacon_api: BeaconApiConfig,
	pub constraints_api: ConstraintsApiConfig,
	pub delegation: DelegationConfig,
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

/// Signing configuration loaded from environment variables
/// This is kept separate from TOML config for security
#[derive(Debug, Clone)]
pub struct SigningConfig {
	/// Legacy single ECDSA private key (for backward compatibility)
	pub private_key: SecretKey,
	/// Multiple key pairs for delegation-based signing
	pub key_pairs: Vec<KeyPair>,
}

/// A single key pair for delegation-based operations
#[derive(Debug, Clone)]
pub struct KeyPair {
	/// Human-readable identifier
	pub name: String,
	/// ECDSA private key for commitment signing
	pub ecdsa_private_key: SecretKey,
	/// BLS private key for constraint signing
	pub bls_private_key: BlsSecretKey,
	/// Corresponding BLS public key
	pub bls_public_key: BlsPublicKey,
	/// Ethereum address derived from ECDSA key
	pub committer_address: String,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			server: ServerConfig::default(),
			database: DatabaseConfig::default(),
			logging: LoggingConfig::default(),
			validation: ValidationConfig::default(),
			beacon_api: BeaconApiConfig::default(),
			constraints_api: ConstraintsApiConfig::default(),
			delegation: DelegationConfig::default(),
			signing: SigningConfig::default(),
		}
	}
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
		Self {
			slasher_address: "0x0000000000000000000000000000000000000000".to_string(),
		}
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
		Self {
			// Default development private key (same as in .env.example)
			private_key: crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
				.expect("Failed to parse default private key"),
			key_pairs: vec![],
		}
	}
}

impl SigningConfig {
	/// Load signing configuration from environment variables
	pub fn load() -> Result<Self> {
		// Load legacy single key for backward compatibility
		let private_key = if let Ok(private_key_hex) = std::env::var("COMMITTER_PRIVATE_KEY") {
			crypto::parse_private_key(&private_key_hex)
				.context("Invalid private key in COMMITTER_PRIVATE_KEY")?
		} else {
			// Use default if not provided
			crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
				.expect("Failed to parse default private key")
		};

		// Load multiple key pairs from environment
		let key_pairs = Self::load_key_pairs()?;

		Ok(Self { private_key, key_pairs })
	}

	/// Load multiple key pairs from environment variables
	/// Expected format: GATEWAY_KEY_PAIRS_COUNT=N
	/// GATEWAY_KEY_PAIR_0_NAME=main
	/// GATEWAY_KEY_PAIR_0_ECDSA=0x...
	/// GATEWAY_KEY_PAIR_0_BLS=0x...
	fn load_key_pairs() -> Result<Vec<KeyPair>> {
		let count = std::env::var("GATEWAY_KEY_PAIRS_COUNT")
			.unwrap_or_else(|_| "0".to_string())
			.parse::<usize>()
			.context("Invalid GATEWAY_KEY_PAIRS_COUNT")?;

		let mut key_pairs = Vec::new();

		for i in 0..count {
			let name = std::env::var(format!("GATEWAY_KEY_PAIR_{}_NAME", i))
				.unwrap_or_else(|_| format!("keypair_{}", i));

			let ecdsa_hex = std::env::var(format!("GATEWAY_KEY_PAIR_{}_ECDSA", i))
				.with_context(|| format!("Missing GATEWAY_KEY_PAIR_{}_ECDSA", i))?;

			let bls_hex = std::env::var(format!("GATEWAY_KEY_PAIR_{}_BLS", i))
				.with_context(|| format!("Missing GATEWAY_KEY_PAIR_{}_BLS", i))?;

			let ecdsa_private_key = crypto::parse_private_key(&ecdsa_hex)
				.with_context(|| format!("Invalid ECDSA key for pair {}", i))?;

			let (bls_private_key, bls_public_key) = Self::parse_bls_key(&bls_hex)
				.with_context(|| format!("Invalid BLS key for pair {}", i))?;

			let committer_address = crypto::ecdsa_to_address(&ecdsa_private_key)
				.with_context(|| format!("Failed to derive address for pair {}", i))?;

			key_pairs.push(KeyPair {
				name,
				ecdsa_private_key,
				bls_private_key,
				bls_public_key,
				committer_address,
			});
		}

		Ok(key_pairs)
	}

	/// Parse BLS private key and derive public key
	fn parse_bls_key(hex_str: &str) -> Result<(BlsSecretKey, BlsPublicKey)> {
		let key_bytes = crypto::parse_hex_bytes(hex_str, 32)
			.context("Invalid BLS private key hex")?;

		let private_key = BlsSecretKey::from_bytes(&key_bytes)
			.map_err(|e| anyhow::anyhow!("Invalid BLS private key: {:?}", e))?;

		let public_key = private_key.sk_to_pk();

		Ok((private_key, public_key))
	}

	/// Find key pair by committer address
	pub fn find_key_pair_by_address(&self, address: &str) -> Option<&KeyPair> {
		self.key_pairs.iter().find(|kp| kp.committer_address == address)
	}

	/// Find key pair by BLS public key
	pub fn find_key_pair_by_bls_pubkey(&self, pubkey: &[u8; 48]) -> Option<&KeyPair> {
		self.key_pairs.iter().find(|kp| {
			kp.bls_public_key.to_bytes().as_slice() == pubkey.as_slice()
		})
	}
}

impl Config {
	pub fn load() -> Result<Self> {
		let mut config = Self::load_from_file("config.toml")?;

		// Load signing config from environment variables
		config.signing = SigningConfig::load()
			.unwrap_or_else(|_| {
				tracing::warn!("Failed to load signing config from environment, using defaults");
				SigningConfig::default()
			});

		Ok(config)
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
