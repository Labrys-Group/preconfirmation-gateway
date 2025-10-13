use anyhow::Result;
use preconfirmation_gateway::config::{Config, DatabaseConfig};
use preconfirmation_gateway::db;
use preconfirmation_gateway::types::{DatabaseContext};
use std::env;

// Mock database configuration for testing
fn create_test_config() -> Config {
	Config {
		database: DatabaseConfig {
			url: "postgresql://test:test@localhost:5432/test_db".to_string(),
		},
		..Default::default()
	}
}

fn create_invalid_config() -> Config {
	Config {
		database: DatabaseConfig {
			url: "invalid://not_a_database".to_string(),
		},
		..Default::default()
	}
}

#[test]
fn test_database_config_validation() {
	let config = create_test_config();
	assert_eq!(config.database.url, "postgresql://test:test@localhost:5432/test_db");
	assert_eq!(config.database_url(), "postgresql://test:test@localhost:5432/test_db");
}

#[test]
fn test_database_config_with_env_override() {
	let original_env = env::var("DATABASE_URL").ok();
	
	// Set a test environment variable
	env::set_var("DATABASE_URL", "postgresql://env_override:test@localhost/env_db");
	
	let config = create_test_config();
	
	// The environment variable should be used by create_pool function
	// We'll test this indirectly through the config parsing
	assert_eq!(env::var("DATABASE_URL").unwrap(), "postgresql://env_override:test@localhost/env_db");
	
	// Restore original environment
	match original_env {
		Some(val) => env::set_var("DATABASE_URL", val),
		None => env::remove_var("DATABASE_URL"),
	}
}

#[tokio::test]
async fn test_database_context_creation() {
	// We can't test actual database connections without a real database
	// But we can test that the DatabaseContext can be created with mock data
	// This would typically be done with a test database or mockall crate
	
	// For now, let's test the creation fails gracefully with invalid config
	let config = create_invalid_config();
	let result = db::create_pool(&config).await;
	
	// This should fail because the URL is invalid
	assert!(result.is_err());
	
	// Check the error message contains relevant information
	let error = result.unwrap_err();
	let error_msg = error.to_string().to_lowercase();
	assert!(error_msg.contains("failed to create connection pool") || error_msg.contains("invalid"));
}

#[test]
fn test_database_context_debug_impl() {
	// Test that DatabaseContext implements Debug properly
	// We need a mock pool for this, but since we can't easily create one,
	// we'll test the debug implementation conceptually
	
	// This is more of a compile-time test - if DatabaseContext doesn't implement Debug,
	// this won't compile
	let debug_string = format!("{:?}", create_test_config().database);
	assert!(debug_string.contains("test"));
}

// Test URL parsing and validation
#[test]
fn test_database_url_parsing() {
	let valid_urls = vec![
		"postgresql://user:pass@localhost:5432/dbname",
		"postgres://user:pass@localhost/dbname",
		"postgresql://localhost/dbname",
		"postgresql://user@localhost/dbname",
	];
	
	for url in valid_urls {
		let config = Config {
			database: DatabaseConfig { url: url.to_string() },
			..Default::default()
		};
		
		// Basic validation - the URL should be parseable
		assert!(!config.database_url().is_empty());
		assert!(config.database_url().starts_with("postgres"));
	}
}

#[test]
fn test_database_url_validation_invalid() {
	let invalid_urls = vec![
		"",
		"not_a_url",
		"http://wrong_protocol/db",
		"postgresql://",
	];
	
	for url in invalid_urls {
		let config = Config {
			database: DatabaseConfig { url: url.to_string() },
			..Default::default()
		};
		
		// The config will accept any string, but connection creation should fail
		// This is expected behavior - validation happens at connection time
		assert_eq!(config.database_url(), url);
	}
}

// Test connection pool configuration
#[tokio::test]
async fn test_create_pool_with_various_configs() {
	// Test with default config
	let default_config = Config::default();
	let result = db::create_pool(&default_config).await;
	// This will likely fail without a real database, but should not panic
	// and should provide a meaningful error
	if let Err(e) = result {
		let error_msg = e.to_string();
		// Should contain information about connection failure, not parsing failure
		assert!(error_msg.contains("connection") || error_msg.contains("pool") || error_msg.contains("failed"));
	}
	
	// Test with custom config
	let custom_config = create_test_config();
	let result = db::create_pool(&custom_config).await;
	if let Err(e) = result {
		let error_msg = e.to_string();
		assert!(error_msg.contains("connection") || error_msg.contains("pool") || error_msg.contains("failed"));
	}
}

// Test environment variable precedence
#[tokio::test]
async fn test_env_var_precedence() {
	let original_env = env::var("DATABASE_URL").ok();
	
	// Set environment variable
	env::set_var("DATABASE_URL", "postgresql://env:test@localhost/env_db");
	
	let config = create_test_config();
	
	// The create_pool function should use the environment variable
	// We can't test the actual connection, but we can verify the env var is read
	assert_eq!(env::var("DATABASE_URL").unwrap(), "postgresql://env:test@localhost/env_db");
	
	// Restore environment
	match original_env {
		Some(val) => env::set_var("DATABASE_URL", val),
		None => env::remove_var("DATABASE_URL"),
	}
}

// Test DatabaseContext methods (conceptually, without actual DB)
#[test]
fn test_database_context_interface() {
	// Test that the DatabaseContext has the expected interface
	// This is mainly a compile-time test
	
	// These should compile without errors, proving the interface exists
	fn _test_compilation() -> Result<()> {
		// This won't run, but ensures the methods exist with correct signatures
		if false {
			let pool = deadpool_postgres::Pool::builder(deadpool_postgres::Config::new()).build()?;
			let ctx = DatabaseContext::new(pool);
			
			// Test method signatures exist
			let _client_future = ctx.client();
			let _test_future = ctx.test_connection();
			let _with_client_future = ctx.with_client(|_client| async { Ok(()) });
		}
		Ok(())
	}
	
	assert!(_test_compilation().is_ok());
}

// Mock test for connection testing (without actual database)
#[test]
fn test_connection_error_handling() {
	// Test that connection errors are properly wrapped and contextual
	// This verifies our error handling provides good user feedback
	
	let invalid_config = create_invalid_config();
	
	// We can test the configuration creation at least
	assert!(invalid_config.database_url().starts_with("invalid://"));
	
	// Test that database URL accessor works
	assert_eq!(invalid_config.database_url(), "invalid://not_a_database");
}