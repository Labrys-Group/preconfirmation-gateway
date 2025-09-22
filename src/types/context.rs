use super::database::DatabaseContext;
use crate::config::Config;

/// RPC context that provides access to shared resources for all RPC handlers
#[derive(Clone, Debug)]
pub struct RpcContext {
	/// Database context for PostgreSQL operations
	pub database: DatabaseContext,
	/// Configuration for validation and other settings
	pub config: Config,
}

impl RpcContext {
	/// Create a new RPC context with the given database context and config
	pub fn new(database: DatabaseContext, config: Config) -> Self {
		Self { database, config }
	}

	/// Convenience method for database operations
	/// This delegates to the underlying DatabaseContext's with_client method
	pub async fn with_database<F, Fut, R>(&self, f: F) -> anyhow::Result<R>
	where
		F: FnOnce(deadpool_postgres::Client) -> Fut,
		Fut: std::future::Future<Output = anyhow::Result<R>>,
	{
		self.database.with_client(f).await
	}

	/// Get a database client from the connection pool
	pub async fn database_client(&self) -> anyhow::Result<deadpool_postgres::Client> {
		self.database.client().await
	}
}
