use jsonrpsee::server::RpcModule;

use super::super::types::RpcContext;
use super::handlers;

/// Builds an `RpcModule` with the crate's RPC methods registered.
///
/// Registers the RPC methods "commitmentRequest", "commitmentResult", "slots", and "fee" on a new
/// `RpcModule` created from the provided `RpcContext`. Returns the configured `RpcModule` on
/// success or an error if any method registration fails.
///
/// # Examples
///
pub fn setup_rpc_methods(rpc_context: RpcContext) -> anyhow::Result<RpcModule<RpcContext>> {
	let mut module = RpcModule::new(rpc_context);

	module.register_async_method("commitmentRequest", handlers::commitment_request_handler)?;
	module.register_async_method("commitmentResult", handlers::commitment_result_handler)?;
	module.register_method("slots", handlers::slots_handler)?;
	module.register_async_method("fee", handlers::fee_handler)?;

	Ok(module)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::Config;
	use crate::testing::helpers::TestHelpers;
	use std::sync::Arc;

	#[tokio::test]
	async fn test_setup_rpc_methods() {
		// Create a test RPC context
		let config = Arc::new(Config::default());
		let rpc_context = TestHelpers::create_test_rpc_context(config);

		// Test that we can create the RPC module without errors
		let module = setup_rpc_methods((*rpc_context).clone());
		assert!(module.is_ok(), "Should be able to set up RPC methods");

		let module = module.unwrap();

		// Verify that the module has the expected methods registered
		let method_names = module.method_names().collect::<Vec<_>>();
		assert_eq!(method_names.len(), 4, "Should register exactly 4 methods");

		// Verify specific method names are registered
		assert!(method_names.contains(&"commitmentRequest"), "Should register commitmentRequest method");
		assert!(method_names.contains(&"commitmentResult"), "Should register commitmentResult method");
		assert!(method_names.contains(&"slots"), "Should register slots method");
		assert!(method_names.contains(&"fee"), "Should register fee method");
	}
}
