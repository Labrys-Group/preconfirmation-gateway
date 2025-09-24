mod config;
mod crypto;
mod db;
mod rpc;
mod server;
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

	// Create RPC context with database context and config
	let rpc_context = types::RpcContext::new(db_context, config.clone());

	server::run_server(rpc_context, &config).await?;

	Ok(())
}
