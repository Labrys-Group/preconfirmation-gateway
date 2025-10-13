use anyhow::Result;
use preconfirmation_gateway::config::{Config, LoggingConfig, ServerConfig};
use preconfirmation_gateway::server::{setup_logging, server_address};
use std::io::Write;
use tempfile::NamedTempFile;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::util::SubscriberInitExt;

#[test]
fn test_setup_logging_basic() -> Result<()> {
	let config = Config::default();
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	Ok(())
}

#[test]
fn test_setup_logging_with_custom_level() -> Result<()> {
	let mut config = Config::default();
	config.logging.level = "debug".to_string();
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	Ok(())
}

#[test]
fn test_setup_logging_with_method_tracing_disabled() -> Result<()> {
	let mut config = Config::default();
	config.logging.enable_method_tracing = false;
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	Ok(())
}

#[test]
fn test_setup_logging_with_custom_traced_methods() -> Result<()> {
	let mut config = Config::default();
	config.logging.traced_methods = vec!["custom_method".to_string(), "another_method".to_string()];
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	Ok(())
}

#[test]
fn test_setup_logging_with_empty_traced_methods() -> Result<()> {
	let mut config = Config::default();
	config.logging.traced_methods = vec![];
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	Ok(())
}

#[test]
fn test_setup_logging_with_env_var() -> Result<()> {
	// Set environment variable for tracing level
	std::env::set_var("RUST_LOG", "warn");
	
	let config = Config::default();
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	// Clean up
	std::env::remove_var("RUST_LOG");
	
	Ok(())
}

#[test]
fn test_setup_logging_invalid_filter_directive() -> Result<()> {
	let mut config = Config::default();
	config.logging.enable_method_tracing = true;
	// Add a method name that might cause issues with filter parsing
	config.logging.traced_methods = vec!["method[with]brackets".to_string()];
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	// This should still work because jsonrpsee should handle the method name
	let result = setup_logging(&config);
	assert!(result.is_ok());
	
	Ok(())
}

#[test]
fn test_server_address_default() {
	let config = Config::default();
	let address = server_address(&config);
	assert_eq!(address, "127.0.0.1:8080");
}

#[test]
fn test_server_address_custom_host_and_port() {
	let config = Config {
		server: ServerConfig {
			host: "0.0.0.0".to_string(),
			port: 9090,
		},
		..Default::default()
	};
	
	let address = server_address(&config);
	assert_eq!(address, "0.0.0.0:9090");
}

#[test]
fn test_server_address_ipv6() {
	let config = Config {
		server: ServerConfig {
			host: "::1".to_string(),
			port: 3000,
		},
		..Default::default()
	};
	
	let address = server_address(&config);
	assert_eq!(address, "::1:3000");
}

#[test]
fn test_server_address_hostname() {
	let config = Config {
		server: ServerConfig {
			host: "localhost".to_string(),
			port: 8080,
		},
		..Default::default()
	};
	
	let address = server_address(&config);
	assert_eq!(address, "localhost:8080");
}

#[test]
fn test_server_address_edge_ports() {
	// Test minimum port
	let config_min = Config {
		server: ServerConfig {
			host: "127.0.0.1".to_string(),
			port: 1,
		},
		..Default::default()
	};
	assert_eq!(server_address(&config_min), "127.0.0.1:1");
	
	// Test maximum port
	let config_max = Config {
		server: ServerConfig {
			host: "127.0.0.1".to_string(),
			port: 65535,
		},
		..Default::default()
	};
	assert_eq!(server_address(&config_max), "127.0.0.1:65535");
}

#[test]
fn test_logging_config_validation() {
	let config = LoggingConfig {
		level: "info".to_string(),
		enable_method_tracing: true,
		traced_methods: vec!["test".to_string()],
	};
	
	// Test that valid log levels work
	let valid_levels = vec!["error", "warn", "info", "debug", "trace"];
	for level in valid_levels {
		let mut test_config = config.clone();
		test_config.level = level.to_string();
		
		// This should not panic or error
		let filter_result = tracing_subscriber::EnvFilter::try_new(&test_config.level);
		assert!(filter_result.is_ok());
	}
}

// Integration test for logging setup with actual log output
#[test]
fn test_logging_integration() -> Result<()> {
	let config = Config::default();
	
	// Reset any existing global subscriber
	let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
	
	// Setup logging
	setup_logging(&config)?;
	
	// Test that we can actually log messages
	tracing::info!("Test log message");
	tracing::debug!("Debug message");
	tracing::error!("Error message");
	
	// If we get here without panicking, the logging setup worked
	Ok(())
}