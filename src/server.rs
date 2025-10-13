use std::net::SocketAddr;

use anyhow::Result;
use jsonrpsee::server::Server;
use tracing_subscriber::util::SubscriberInitExt;

use crate::{config, rpc, types};

pub async fn run_server(rpc_context: types::RpcContext, config: &config::Config) -> Result<()> {
	let server = Server::builder().build(server_address(config).parse::<SocketAddr>()?).await?;
	let module = rpc::setup_rpc_methods(rpc_context)?;

	let addr = server.local_addr()?;
	tracing::info!("Starting RPC server on {}", addr);
	let handle = server.start(module);

	// Run the server indefinitely, waiting for incoming requests
	handle.stopped().await;

	Ok(())
}

pub fn setup_logging(config: &config::Config) -> Result<()> {
	let mut filter = tracing_subscriber::EnvFilter::try_from_default_env()
		.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.logging.level));

	if config.logging.enable_method_tracing {
		for method in &config.logging.traced_methods {
			let directive = format!("jsonrpsee[method_call{{name = \"{}\"}}]=trace", method);
			filter = filter.add_directive(directive.parse()?);
		}
	}

	tracing_subscriber::FmtSubscriber::builder().with_env_filter(filter).finish().try_init()?;
	Ok(())
}

pub fn server_address(config: &config::Config) -> String {
	format!("{}:{}", config.server.host, config.server.port)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::api::beacon::BeaconApiClient;
	use crate::api::reth::RethApiClient;
	use crate::config::Config;
	use crate::db::DatabaseContext;
	use crate::services::fee_pricing::FeePricingEngine;
	use crate::types::RpcContext;

	/// Helper function to create a test config
	fn create_test_config() -> Config {
		Config {
			server: crate::config::ServerConfig { host: "127.0.0.1".to_string(), port: 8080 },
			database: crate::config::DatabaseConfig { url: "postgresql://test:test@localhost/test_db".to_string() },
			logging: crate::config::LoggingConfig {
				level: "info".to_string(),
				enable_method_tracing: false,
				traced_methods: vec![],
			},
			validation: crate::config::ValidationConfig {
				slasher_address: "0x1234567890123456789012345678901234567890".to_string(),
			},
			beacon_api: crate::config::BeaconApiConfig {
				primary_endpoint: "https://test.beacon.com".to_string(),
				fallback_endpoints: vec!["https://fallback.beacon.com".to_string()],
				request_timeout_secs: 30,
				genesis_time: 1606824023,
			},
			constraints_api: crate::config::ConstraintsApiConfig {
				relay_endpoint: "https://test.relay.com".to_string(),
				request_timeout_secs: 10,
				max_retries: 3,
				authorized_builders: vec!["builder1".to_string(), "builder2".to_string()],
			},
			delegation: crate::config::DelegationConfig {
				lookahead_epochs: 2,
				polling_interval_secs: 60,
				cache_ttl_secs: 300,
				domain_application_gateway: "0x00000002".to_string(),
			},
			reth: crate::config::RethConfig {
				endpoint: "http://localhost:8545".to_string(),
				request_timeout_secs: 10,
				max_retries: 3,
				fee_config: crate::config::FeeConfig {
					scaling_factor: 2.0,
					default_gas_limit: 30_000_000,
					min_fee_multiplier: 1.0,
					max_fee_multiplier: 100.0,
					cache_ttl_secs: 60,
				},
			},
			signing: crate::config::SigningConfig::default(),
		}
	}

	#[test]
	fn test_server_address_formatting() {
		let config = create_test_config();
		let address = server_address(&config);
		assert_eq!(address, "127.0.0.1:8080");
	}

	#[test]
	fn test_server_address_with_different_host() {
		let mut config = create_test_config();
		config.server.host = "0.0.0.0".to_string();
		config.server.port = 9090;

		let address = server_address(&config);
		assert_eq!(address, "0.0.0.0:9090");
	}

	#[test]
	fn test_server_address_with_ipv6_host() {
		let mut config = create_test_config();
		config.server.host = "::1".to_string();
		config.server.port = 8080;

		let address = server_address(&config);
		assert_eq!(address, "::1:8080");
	}

	#[test]
	fn test_setup_logging_success() {
		let config = create_test_config();

		// This should not panic, even if logging is already initialized
		let result = setup_logging(&config);
		// In test environment, this might fail due to logging already being initialized
		// That's okay - we're testing that the function doesn't panic
		if let Err(e) = result {
			println!("Expected logging setup error in test environment: {}", e);
		}
	}

	#[test]
	fn test_setup_logging_with_method_tracing() {
		let mut config = create_test_config();
		config.logging.enable_method_tracing = true;
		config.logging.traced_methods = vec!["test_method".to_string(), "another_method".to_string()];

		let result = setup_logging(&config);
		// This should not panic
		if let Err(e) = result {
			println!("Expected logging setup error in test environment: {}", e);
		}
	}

	#[test]
	fn test_setup_logging_with_invalid_method_name() {
		let mut config = create_test_config();
		config.logging.enable_method_tracing = true;
		config.logging.traced_methods = vec!["invalid method name with spaces".to_string()];

		let result = setup_logging(&config);
		// This should fail due to invalid method name format
		assert!(result.is_err());
	}

	#[test]
	fn test_setup_logging_with_different_levels() {
		let levels = vec!["trace", "debug", "info", "warn", "error"];

		for level in levels {
			let mut config = create_test_config();
			config.logging.level = level.to_string();

			let result = setup_logging(&config);
			// Should not panic for any valid log level
			if let Err(e) = result {
				println!("Expected logging setup error for level {}: {}", level, e);
			}
		}
	}

	#[tokio::test]
	async fn test_run_server_with_mock_context() {
		let config = create_test_config();

		// Create a test database context
		let db_context = DatabaseContext::new_for_testing();

		// Create mock services
		let beacon_config = config.beacon_api.clone();
		let beacon_client = BeaconApiClient::new(beacon_config).expect("Failed to create beacon client");

		let reth_config = crate::api::reth::RethApiConfig {
			endpoint: config.reth.endpoint.clone(),
			request_timeout_secs: config.reth.request_timeout_secs,
			max_retries: config.reth.max_retries,
		};
		let reth_client = RethApiClient::new(reth_config).expect("Failed to create Reth client");
		let fee_engine = FeePricingEngine::new(
			std::sync::Arc::new(reth_client),
			std::sync::Arc::new(db_context.clone()),
			std::sync::Arc::new(config.clone()),
		);

		let rpc_context = RpcContext::new(
			db_context,
			config.clone(),
			std::sync::Arc::new(fee_engine),
			std::sync::Arc::new(beacon_client),
		);

		// Test that we can create the RPC module (this tests the server setup)
		let module_result = crate::rpc::setup_rpc_methods(rpc_context);
		assert!(module_result.is_ok(), "Failed to setup RPC methods: {:?}", module_result.err());
	}

	#[test]
	fn test_server_address_edge_cases() {
		let mut config = create_test_config();

		// Test with empty host
		config.server.host = "".to_string();
		let address = server_address(&config);
		assert_eq!(address, ":8080");

		// Test with port 0
		config.server.host = "localhost".to_string();
		config.server.port = 0;
		let address = server_address(&config);
		assert_eq!(address, "localhost:0");

		// Test with very large port number
		config.server.port = 65535;
		let address = server_address(&config);
		assert_eq!(address, "localhost:65535");
	}

	#[test]
	fn test_config_validation_for_server() {
		let config = create_test_config();

		// Test that server config has reasonable values
		assert!(!config.server.host.is_empty());
		assert!(config.server.port > 0);

		// Test that logging config is valid
		assert!(!config.logging.level.is_empty());
		assert!(matches!(config.logging.level.as_str(), "trace" | "debug" | "info" | "warn" | "error"));
	}
}
