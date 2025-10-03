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

	// Initialize beacon API client (needed for RPC context and background services)
	let beacon_client = std::sync::Arc::new(
		api::beacon::BeaconApiClient::new(config.beacon_api.clone())?
	);

	// Create RPC context with database context, config, fee engine, and beacon client
	let rpc_context = types::RpcContext::new(
		db_context.clone(),
		config.clone(),
		fee_engine,
		beacon_client.clone(),
	);
	let constraints_client = std::sync::Arc::new(
		api::constraints::ConstraintsApiClient::new(config.constraints_api.clone())?
	);

	// Start delegation polling service
	let delegation_service = services::delegation_polling::DelegationPollingService::new(
		beacon_client.clone(),
		constraints_client.clone(),
		std::sync::Arc::new(db_context.pool().clone()),
		std::sync::Arc::new(config.clone()),
	).await?;

	delegation_service.start().await?;
	tracing::info!("Delegation polling service started");

	// Trigger an immediate poll to fetch delegations on startup
	if let Err(e) = delegation_service.poll_once().await {
		tracing::warn!("Initial delegation poll failed: {}", e);
	}

	// Run the RPC server (this blocks until shutdown)
	server::run_server(rpc_context, &config).await?;

	Ok(())
}
