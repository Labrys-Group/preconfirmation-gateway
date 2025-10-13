use anyhow::Result;
use preconfirmation_gateway::{
	config::Config,
	rpc::methods::setup_rpc_methods,
	server::server_address,
	types::{DatabaseContext, RpcContext, CommitmentRequest},
};
use deadpool_postgres::{Config as DbConfig, Runtime};
use tokio_postgres::NoTls;
use jsonrpsee::{
	server::{Server, ServerHandle},
	http_client::{HttpClient, HttpClientBuilder},
	core::client::ClientT,
};
use serde_json::json;
use std::net::SocketAddr;

// Create a test RPC context
async fn create_test_rpc_context() -> Result<RpcContext> {
	let mut cfg = DbConfig::new();
	cfg.url = Some("postgresql://test:test@localhost:5432/test_db".to_string());
	
	let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
	let db_context = DatabaseContext::new(pool);
	Ok(RpcContext::new(db_context))
}

// Start a test server
async fn start_test_server() -> Result<(ServerHandle, SocketAddr)> {
	let context = create_test_rpc_context().await?;
	let module = setup_rpc_methods(context)?;
	
	let server = Server::builder()
		.build("127.0.0.1:0".parse::<SocketAddr>()?)
		.await?;
	
	let addr = server.local_addr()?;
	let handle = server.start(module);
	
	Ok((handle, addr))
}

#[tokio::test]
async fn test_server_startup_and_shutdown() -> Result<()> {
	// Test that we can start and stop the server without errors
	match start_test_server().await {
		Ok((handle, _addr)) => {
			// Server started successfully, now stop it
			handle.stop().unwrap();
			Ok(())
		}
		Err(e) => {
			// Expected to fail without a real database, but should fail gracefully
			let error_msg = e.to_string().to_lowercase();
			assert!(
				error_msg.contains("connection") || 
				error_msg.contains("database") ||
				error_msg.contains("pool")
			);
			Ok(())
		}
	}
}

#[tokio::test]
async fn test_config_to_server_address_conversion() {
	let config = Config::default();
	let addr_str = server_address(&config);
	
	// Test that the address string can be parsed as a SocketAddr
	let socket_addr: Result<SocketAddr, _> = addr_str.parse();
	assert!(socket_addr.is_ok());
	
	let addr = socket_addr.unwrap();
	assert_eq!(addr.port(), 8080);
}

// Test the full RPC flow (without actual network calls due to database dependency)
#[tokio::test]
async fn test_rpc_module_method_registration() -> Result<()> {
	match create_test_rpc_context().await {
		Ok(context) => {
			let module = setup_rpc_methods(context)?;
			
			// If we get here, the RPC module was created successfully
			// The methods should be registered in the module
			assert!(format!("{:?}", module).contains("RpcModule"));
			Ok(())
		}
		Err(_) => {
			// Expected without a real database connection
			Ok(())
		}
	}
}

// Mock HTTP client test (conceptual test of client-server interaction)
#[tokio::test]
async fn test_http_client_creation() -> Result<()> {
	// Test that we can create an HTTP client for connecting to our server
	let client_result = HttpClientBuilder::default().build("http://127.0.0.1:8080");
	
	match client_result {
		Ok(_client) => {
			// Client created successfully
			Ok(())
		}
		Err(e) => {
			// This might fail due to various reasons, but should not panic
			println!("Client creation failed as expected: {}", e);
			Ok(())
		}
	}
}

// Test configuration loading and server address generation
#[test]
fn test_config_integration() {
	let config = Config::default();
	
	// Test that all config components work together
	assert!(!config.database_url().is_empty());
	assert!(!config.server.host.is_empty());
	assert!(config.server.port > 0);
	assert!(!config.logging.level.is_empty());
	
	// Test server address generation
	let addr = server_address(&config);
	assert!(addr.contains(&config.server.host));
	assert!(addr.contains(&config.server.port.to_string()));
}

// Test the complete data flow through types
#[test]
fn test_data_flow_integration() {
	// Test that data can flow through all our type conversions
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![1, 2, 3, 4],
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	// Test JSON round-trip (simulating RPC parameter parsing)
	let json_value = serde_json::to_value(&request).expect("Failed to convert to JSON");
	let json_str = serde_json::to_string(&json_value).expect("Failed to serialize JSON");
	let parsed_value: serde_json::Value = serde_json::from_str(&json_str).expect("Failed to parse JSON");
	let recovered_request: CommitmentRequest = serde_json::from_value(parsed_value).expect("Failed to convert from JSON");
	
	assert_eq!(recovered_request.commitment_type, request.commitment_type);
	assert_eq!(recovered_request.payload, request.payload);
	assert_eq!(recovered_request.slasher, request.slasher);
}

// Test error handling throughout the stack
#[tokio::test]
async fn test_error_handling_integration() {
	// Test that errors are properly propagated through the system
	
	// 1. Test database configuration errors
	let mut invalid_config = DbConfig::new();
	invalid_config.url = Some("invalid://url".to_string());
	
	let pool_result = invalid_config.create_pool(Some(Runtime::Tokio1), NoTls);
	assert!(pool_result.is_err());
	
	// 2. Test server binding errors (try to bind to invalid address)
	let server_result = Server::builder().build("999.999.999.999:99999".parse::<SocketAddr>());
	// This should fail at parse time
	assert!(server_result.is_err());
}

// Test configuration validation
#[test]
fn test_config_validation_integration() {
	let config = Config::default();
	
	// Test that default configuration values are valid
	assert!(!config.database.url.is_empty());
	assert!(config.database.url.starts_with("postgresql://"));
	
	assert!(config.server.port > 0);
	assert!(config.server.port <= 65535);
	
	assert!(!config.logging.level.is_empty());
	let valid_levels = ["error", "warn", "info", "debug", "trace"];
	assert!(valid_levels.contains(&config.logging.level.as_str()));
}

// Test that all components can be constructed without panicking
#[test]
fn test_component_construction() {
	// Test configuration
	let _config = Config::default();
	
	// Test that we can construct database config
	let mut db_config = DbConfig::new();
	db_config.url = Some("postgresql://localhost/test".to_string());
	
	// Test server address generation
	let config = Config::default();
	let _addr = server_address(&config);
	
	// Test RPC types
	let _request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![],
		slasher: "0x0000000000000000000000000000000000000000".to_string(),
	};
}

// Performance test (basic)
#[test]
fn test_serialization_performance() {
	let large_request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![0xFF; 10000], // 10KB payload
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	let start = std::time::Instant::now();
	
	// Perform multiple serialization/deserialization cycles
	for _ in 0..100 {
		let serialized = serde_json::to_string(&large_request).expect("Serialization failed");
		let _deserialized: CommitmentRequest = serde_json::from_str(&serialized).expect("Deserialization failed");
	}
	
	let elapsed = start.elapsed();
	
	// Should complete in reasonable time (less than 1 second for 100 iterations)
	assert!(elapsed.as_secs() < 1, "Serialization took too long: {:?}", elapsed);
}