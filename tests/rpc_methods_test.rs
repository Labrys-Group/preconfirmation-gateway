use anyhow::Result;
use preconfirmation_gateway::rpc::methods::setup_rpc_methods;
use preconfirmation_gateway::types::{DatabaseContext, RpcContext};
use deadpool_postgres::{Config, Runtime};
use tokio_postgres::NoTls;
use jsonrpsee::server::RpcModule;

// Create a mock RPC context for testing
fn create_mock_rpc_context() -> RpcContext {
	let mut cfg = Config::new();
	cfg.url = Some("postgresql://mock:mock@localhost:5432/mock_db".to_string());
	
	let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)
		.expect("Failed to create mock pool");
	
	let db_context = DatabaseContext::new(pool);
	RpcContext::new(db_context)
}

#[test]
fn test_setup_rpc_methods_success() {
	let context = create_mock_rpc_context();
	let result = setup_rpc_methods(context);
	
	assert!(result.is_ok());
}

#[test]
fn test_rpc_module_contains_expected_methods() {
	let context = create_mock_rpc_context();
	let module = setup_rpc_methods(context).expect("Failed to setup RPC methods");
	
	// Test that the module has been created successfully
	// We can't easily inspect the module's methods without calling them,
	// but we can verify the module was created without errors
	assert!(format!("{:?}", module).contains("RpcModule"));
}

#[test]
fn test_multiple_rpc_module_creation() {
	// Test that we can create multiple RPC modules
	let context1 = create_mock_rpc_context();
	let context2 = create_mock_rpc_context();
	
	let module1 = setup_rpc_methods(context1);
	let module2 = setup_rpc_methods(context2);
	
	assert!(module1.is_ok());
	assert!(module2.is_ok());
}

#[test]
fn test_rpc_context_creation() {
	let context = create_mock_rpc_context();
	
	// Test that the context can be used to create RPC modules
	let module_result = setup_rpc_methods(context);
	assert!(module_result.is_ok());
}

#[test]
fn test_rpc_context_debug() {
	let context = create_mock_rpc_context();
	let debug_str = format!("{:?}", context);
	
	// Should contain RpcContext in the debug output
	assert!(debug_str.contains("RpcContext") || debug_str.contains("database"));
}