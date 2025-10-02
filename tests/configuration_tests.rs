//! Configuration system tests
//!
//! These tests verify the configuration loading and validation including:
//! - Multi-key configuration loading from environment variables
//! - Configuration file parsing and defaults
//! - Key pair validation and mapping
//! - Error handling for invalid configurations

use anyhow::Result;
use preconfirmation_gateway::config::{
	BeaconApiConfig, Config, ConstraintsApiConfig, DelegationConfig, KeyPair, SigningConfig,
	ValidationConfig,
};
use preconfirmation_gateway::crypto::{bls_keys, parse_private_key, ecdsa_to_address};
use serde_json::json;
use std::env;
use tempfile::NamedTempFile;

#[test]
fn test_default_configuration() {
	let config = Config::default();

	// Test server defaults
	assert_eq!(config.server.host, "127.0.0.1");
	assert_eq!(config.server.port, 8080);

	// Test database defaults
	assert_eq!(
		config.database.url,
		"postgresql://localhost/preconfirmation_gateway"
	);

	// Test validation defaults
	assert_eq!(
		config.validation.slasher_address,
		"0x0000000000000000000000000000000000000000"
	);

	// Test beacon API defaults
	assert!(config.beacon_api.primary_endpoint.contains("alchemy.com"));
	assert_eq!(config.beacon_api.fallback_endpoints.len(), 0);
	assert_eq!(config.beacon_api.request_timeout_secs, 30);

	// Test constraints API defaults
	assert_eq!(config.constraints_api.relay_endpoint, "https://relay.example.com");
	assert_eq!(config.constraints_api.max_retries, 3);
	assert_eq!(config.constraints_api.authorized_builders.len(), 0);

	// Test delegation defaults
	assert_eq!(config.delegation.lookahead_epochs, 2);
	assert_eq!(config.delegation.polling_interval_secs, 60);
	assert_eq!(config.delegation.domain_application_gateway, "0x00000002");

	// Test signing defaults
	assert_eq!(config.signing.key_pairs.len(), 0);
}

#[test]
fn test_individual_config_sections() {
	// Test BeaconApiConfig
	let beacon_config = BeaconApiConfig {
		primary_endpoint: "https://test-beacon.com".to_string(),
		fallback_endpoints: vec!["https://fallback1.com".to_string(), "https://fallback2.com".to_string()],
		request_timeout_secs: 60,
		genesis_time: 1234567890,
	};

	assert_eq!(beacon_config.primary_endpoint, "https://test-beacon.com");
	assert_eq!(beacon_config.fallback_endpoints.len(), 2);
	assert_eq!(beacon_config.request_timeout_secs, 60);

	// Test ConstraintsApiConfig
	let constraints_config = ConstraintsApiConfig {
		relay_endpoint: "https://test-relay.com".to_string(),
		request_timeout_secs: 15,
		max_retries: 5,
		authorized_builders: vec![
			"0x1111111111111111111111111111111111111111".to_string(),
			"0x2222222222222222222222222222222222222222".to_string(),
		],
	};

	assert_eq!(constraints_config.relay_endpoint, "https://test-relay.com");
	assert_eq!(constraints_config.max_retries, 5);
	assert_eq!(constraints_config.authorized_builders.len(), 2);

	// Test DelegationConfig
	let delegation_config = DelegationConfig {
		lookahead_epochs: 5,
		polling_interval_secs: 30,
		cache_ttl_secs: 600,
		domain_application_gateway: "0x12345678".to_string(),
	};

	assert_eq!(delegation_config.lookahead_epochs, 5);
	assert_eq!(delegation_config.polling_interval_secs, 30);
	assert_eq!(delegation_config.domain_application_gateway, "0x12345678");
}

#[tokio::test]
async fn test_key_pair_creation_and_validation() {
	// Generate test key pairs
	let (bls_private_key, bls_public_key) = bls_keys::generate_keypair();
	let ecdsa_private_key = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
		.expect("Failed to parse test private key");

	let committer_address = ecdsa_to_address(&ecdsa_private_key).unwrap();

	let key_pair = KeyPair {
		name: "test_key".to_string(),
		ecdsa_private_key: ecdsa_private_key.clone(),
		bls_private_key: bls_private_key.clone(),
		bls_public_key: bls_public_key.clone(),
		committer_address: committer_address.clone(),
	};

	// Verify key pair properties
	assert_eq!(key_pair.name, "test_key");
	assert_eq!(key_pair.committer_address, committer_address);

	// Verify BLS key consistency
	let derived_public_key = key_pair.bls_private_key.sk_to_pk();
	assert_eq!(
		derived_public_key.to_bytes(),
		key_pair.bls_public_key.to_bytes()
	);

	// Verify ECDSA address derivation
	let derived_address = ecdsa_to_address(&key_pair.ecdsa_private_key).unwrap();
	assert_eq!(derived_address, key_pair.committer_address);
}

#[tokio::test]
async fn test_signing_config_key_pair_lookup() {
	// Create mock signing config with multiple key pairs
	let (bls_private_key1, bls_public_key1) = bls_keys::generate_keypair();
	let (bls_private_key2, bls_public_key2) = bls_keys::generate_keypair();

	let ecdsa_private_key1 = parse_private_key("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
	let ecdsa_private_key2 = parse_private_key("59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d").unwrap();

	let committer_address1 = ecdsa_to_address(&ecdsa_private_key1).unwrap();
	let committer_address2 = ecdsa_to_address(&ecdsa_private_key2).unwrap();

	let key_pair1 = KeyPair {
		name: "key_pair_1".to_string(),
		ecdsa_private_key: ecdsa_private_key1,
		bls_private_key: bls_private_key1,
		bls_public_key: bls_public_key1.clone(),
		committer_address: committer_address1.clone(),
	};

	let key_pair2 = KeyPair {
		name: "key_pair_2".to_string(),
		ecdsa_private_key: ecdsa_private_key2,
		bls_private_key: bls_private_key2,
		bls_public_key: bls_public_key2.clone(),
		committer_address: committer_address2.clone(),
	};

	let signing_config = SigningConfig {
		private_key: ecdsa_private_key1,
		key_pairs: vec![key_pair1.clone(), key_pair2.clone()],
	};

	// Test lookup by address
	let found_by_address1 = signing_config.find_key_pair_by_address(&committer_address1);
	assert!(found_by_address1.is_some());
	assert_eq!(found_by_address1.unwrap().name, "key_pair_1");

	let found_by_address2 = signing_config.find_key_pair_by_address(&committer_address2);
	assert!(found_by_address2.is_some());
	assert_eq!(found_by_address2.unwrap().name, "key_pair_2");

	// Test lookup by BLS public key
	let bls_pubkey1_bytes = bls_keys::pubkey_to_bytes(&bls_public_key1);
	let found_by_bls1 = signing_config.find_key_pair_by_bls_pubkey(&bls_pubkey1_bytes);
	assert!(found_by_bls1.is_some());
	assert_eq!(found_by_bls1.unwrap().name, "key_pair_1");

	let bls_pubkey2_bytes = bls_keys::pubkey_to_bytes(&bls_public_key2);
	let found_by_bls2 = signing_config.find_key_pair_by_bls_pubkey(&bls_pubkey2_bytes);
	assert!(found_by_bls2.is_some());
	assert_eq!(found_by_bls2.unwrap().name, "key_pair_2");

	// Test lookup with non-existent keys
	let nonexistent_address = "0x0000000000000000000000000000000000000000";
	assert!(signing_config.find_key_pair_by_address(nonexistent_address).is_none());

	let nonexistent_bls_key = [0u8; 48];
	assert!(signing_config.find_key_pair_by_bls_pubkey(&nonexistent_bls_key).is_none());
}

#[test]
fn test_configuration_serialization() {
	let config = Config::default();

	// Test TOML serialization (excluding signing config which is skipped)
	let toml_str = toml::to_string(&config).unwrap();
	assert!(toml_str.contains("[server]"));
	assert!(toml_str.contains("[database]"));
	assert!(toml_str.contains("[logging]"));
	assert!(toml_str.contains("[validation]"));
	assert!(toml_str.contains("[beacon_api]"));
	assert!(toml_str.contains("[constraints_api]"));
	assert!(toml_str.contains("[delegation]"));

	// Signing config should be skipped in serialization
	assert!(!toml_str.contains("[signing]"));

	// Test deserialization
	let parsed_config: Config = toml::from_str(&toml_str).unwrap();
	assert_eq!(parsed_config.server.host, config.server.host);
	assert_eq!(parsed_config.database.url, config.database.url);
	assert_eq!(parsed_config.delegation.lookahead_epochs, config.delegation.lookahead_epochs);
}

#[test]
fn test_validation_config_address_format() {
	let valid_config = ValidationConfig {
		slasher_address: "0x1234567890123456789012345678901234567890".to_string(),
	};

	// This would be validated by the actual application logic
	assert_eq!(valid_config.slasher_address.len(), 42); // 0x + 40 hex chars
	assert!(valid_config.slasher_address.starts_with("0x"));
}

#[test]
fn test_delegation_config_domain_parsing() {
	use preconfirmation_gateway::crypto::bls::domains;

	let delegation_config = DelegationConfig {
		lookahead_epochs: 2,
		polling_interval_secs: 60,
		cache_ttl_secs: 300,
		domain_application_gateway: "0x12345678".to_string(),
	};

	// Test domain parsing
	let parsed_domain = domains::parse_application_gateway_domain(&delegation_config.domain_application_gateway);
	assert!(parsed_domain.is_ok());

	let domain_bytes = parsed_domain.unwrap();
	assert_eq!(domain_bytes, [0x12, 0x34, 0x56, 0x78]);

	// Test invalid domain
	let invalid_config = DelegationConfig {
		lookahead_epochs: 2,
		polling_interval_secs: 60,
		cache_ttl_secs: 300,
		domain_application_gateway: "invalid_hex".to_string(),
	};

	let invalid_domain = domains::parse_application_gateway_domain(&invalid_config.domain_application_gateway);
	assert!(invalid_domain.is_err());
}

// Note: Environment variable tests are commented out to avoid interfering with the test environment
// In a real test suite, these would be run in isolation with proper environment setup

#[test]
#[ignore = "modifies environment variables"]
fn test_signing_config_environment_loading() {
	// This test would verify loading signing configuration from environment variables:
	//
	// // Set test environment variables
	// env::set_var("GATEWAY_KEY_PAIRS_COUNT", "2");
	// env::set_var("GATEWAY_KEY_PAIR_0_NAME", "validator_1");
	// env::set_var("GATEWAY_KEY_PAIR_0_ECDSA", "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80");
	// env::set_var("GATEWAY_KEY_PAIR_0_BLS", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
	// env::set_var("GATEWAY_KEY_PAIR_1_NAME", "validator_2");
	// env::set_var("GATEWAY_KEY_PAIR_1_ECDSA", "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d");
	// env::set_var("GATEWAY_KEY_PAIR_1_BLS", "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210");
	//
	// let signing_config = SigningConfig::load().unwrap();
	// assert_eq!(signing_config.key_pairs.len(), 2);
	// assert_eq!(signing_config.key_pairs[0].name, "validator_1");
	// assert_eq!(signing_config.key_pairs[1].name, "validator_2");
	//
	// // Cleanup
	// env::remove_var("GATEWAY_KEY_PAIRS_COUNT");
	// env::remove_var("GATEWAY_KEY_PAIR_0_NAME");
	// env::remove_var("GATEWAY_KEY_PAIR_0_ECDSA");
	// env::remove_var("GATEWAY_KEY_PAIR_0_BLS");
	// env::remove_var("GATEWAY_KEY_PAIR_1_NAME");
	// env::remove_var("GATEWAY_KEY_PAIR_1_ECDSA");
	// env::remove_var("GATEWAY_KEY_PAIR_1_BLS");
}

#[test]
fn test_config_file_loading() {
	// Create a temporary config file
	let config_content = r#"
[server]
host = "0.0.0.0"
port = 9090

[database]
url = "postgresql://testhost/testdb"

[logging]
level = "debug"
enable_method_tracing = false
traced_methods = ["testMethod"]

[validation]
slasher_address = "0x9999999999999999999999999999999999999999"

[beacon_api]
primary_endpoint = "https://test-beacon.example.com"
fallback_endpoints = ["https://fallback-beacon.example.com"]
request_timeout_secs = 45
genesis_time = 1234567890

[constraints_api]
relay_endpoint = "https://test-relay.example.com"
request_timeout_secs = 20
max_retries = 5
authorized_builders = ["0x1111111111111111111111111111111111111111"]

[delegation]
lookahead_epochs = 3
polling_interval_secs = 45
cache_ttl_secs = 600
domain_application_gateway = "0xabcdefgh"
"#;

	// Create temporary file
	let mut temp_file = NamedTempFile::new().unwrap();
	std::io::Write::write_all(&mut temp_file, config_content.as_bytes()).unwrap();

	// Test loading configuration from file
	let config = Config::load_from_file(temp_file.path()).unwrap();

	// Verify loaded values
	assert_eq!(config.server.host, "0.0.0.0");
	assert_eq!(config.server.port, 9090);
	assert_eq!(config.database.url, "postgresql://testhost/testdb");
	assert_eq!(config.logging.level, "debug");
	assert!(!config.logging.enable_method_tracing);
	assert_eq!(config.validation.slasher_address, "0x9999999999999999999999999999999999999999");
	assert_eq!(config.beacon_api.primary_endpoint, "https://test-beacon.example.com");
	assert_eq!(config.beacon_api.fallback_endpoints.len(), 1);
	assert_eq!(config.constraints_api.relay_endpoint, "https://test-relay.example.com");
	assert_eq!(config.constraints_api.max_retries, 5);
	assert_eq!(config.delegation.lookahead_epochs, 3);
	assert_eq!(config.delegation.domain_application_gateway, "0xabcdefgh");
}

#[test]
fn test_config_validation() {
	// Test configuration validation logic
	let config = Config::default();

	// Test database URL format
	assert!(!config.database.url.is_empty());
	assert!(config.database.url.starts_with("postgresql://"));

	// Test server configuration
	assert!(!config.server.host.is_empty());
	assert!(config.server.port > 0);
	assert!(config.server.port < 65536);

	// Test beacon API configuration
	assert!(!config.beacon_api.primary_endpoint.is_empty());
	assert!(config.beacon_api.request_timeout_secs > 0);
	assert!(config.beacon_api.genesis_time > 0);

	// Test constraints API configuration
	assert!(!config.constraints_api.relay_endpoint.is_empty());
	assert!(config.constraints_api.request_timeout_secs > 0);
	assert!(config.constraints_api.max_retries > 0);

	// Test delegation configuration
	assert!(config.delegation.lookahead_epochs > 0);
	assert!(config.delegation.polling_interval_secs > 0);
	assert!(config.delegation.cache_ttl_secs > 0);
	assert!(!config.delegation.domain_application_gateway.is_empty());
}

#[test]
fn test_config_error_handling() {
	// Test loading from non-existent file (should use defaults)
	let non_existent_path = "/tmp/non_existent_config.toml";
	let config = Config::load_from_file(non_existent_path);
	assert!(config.is_ok());

	// Test loading invalid TOML
	let invalid_config_content = r#"
[server
host = "invalid toml
"#;

	let mut invalid_temp_file = NamedTempFile::new().unwrap();
	std::io::Write::write_all(&mut invalid_temp_file, invalid_config_content.as_bytes()).unwrap();

	let invalid_config = Config::load_from_file(invalid_temp_file.path());
	assert!(invalid_config.is_err());
}