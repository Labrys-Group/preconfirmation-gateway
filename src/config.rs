use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use secp256k1::SecretKey;

use crate::crypto;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
	pub server: ServerConfig,
	pub database: DatabaseConfig,
	pub logging: LoggingConfig,
	pub validation: ValidationConfig,
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

/// Signing configuration loaded from environment variables
/// This is kept separate from TOML config for security
#[derive(Debug, Clone)]
pub struct SigningConfig {
	pub private_key: SecretKey,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			server: ServerConfig::default(),
			database: DatabaseConfig::default(),
			logging: LoggingConfig::default(),
			validation: ValidationConfig::default(),
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

impl Default for SigningConfig {
	fn default() -> Self {
		Self {
			// Default development private key (same as in .env.example)
			private_key: crypto::parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
				.expect("Failed to parse default private key"),
		}
	}
}

impl SigningConfig {
	/// Load signing configuration from environment variables
	pub fn load() -> Result<Self> {
		let private_key_hex = std::env::var("COMMITTER_PRIVATE_KEY")
			.context("COMMITTER_PRIVATE_KEY environment variable not set")?;

		let private_key = crypto::parse_private_key(&private_key_hex)
			.context("Invalid private key in COMMITTER_PRIVATE_KEY")?;

		Ok(Self { private_key })
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
