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
/// ```ignore
/// // Construct an appropriate RpcContext for your environment.
/// let ctx = /* RpcContext::new(...) */ unimplemented!();
/// let module = setup_rpc_methods(ctx).expect("failed to register RPC methods");
/// // `module` is ready to be served by a jsonrpsee server.
/// ```ignore
pub fn setup_rpc_methods(rpc_context: RpcContext) -> anyhow::Result<RpcModule<RpcContext>> {
	let mut module = RpcModule::new(rpc_context);

	module.register_async_method("commitmentRequest", handlers::commitment_request_handler)?;
	module.register_async_method("commitmentResult", handlers::commitment_result_handler)?;
	module.register_method("slots", handlers::slots_handler)?;
	module.register_async_method("fee", handlers::fee_handler)?;

	Ok(module)
}
