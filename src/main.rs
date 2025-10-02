mod api;
mod config;
mod crypto;
mod db;
mod rpc;
mod server;
mod services;
mod types;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	// Load configuration
	let config = config::Config::load()?;

	// Setup logging with configuration
	server::setup_logging(&config)?;

	// Initialize database connection pool
	let db_pool = db::create_pool(&config).await?;
	db::test_connection(&db_pool).await?;
	let db_context = db::DatabaseContext::new(db_pool);

	// Initialize Reth API client
	let reth_api_config = api::reth::RethApiConfig {
		endpoint: config.reth.endpoint.clone(),
		request_timeout_secs: config.reth.request_timeout_secs,
		max_retries: config.reth.max_retries,
	};
	let reth_client = std::sync::Arc::new(
		api::reth::RethApiClient::new(reth_api_config)?
	);

	// Initialize fee pricing engine
	let fee_engine = std::sync::Arc::new(
		services::fee_pricing::FeePricingEngine::new(
			reth_client,
			std::sync::Arc::new(db_context.clone()),
			std::sync::Arc::new(config.clone()),
		)
	);

	// Create RPC context with database context, config, and fee engine
	let rpc_context = types::RpcContext::new(db_context, config.clone(), fee_engine);

	server::run_server(rpc_context, &config).await?;

	Ok(())
}
