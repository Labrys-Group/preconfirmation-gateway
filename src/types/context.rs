use crate::{config::Config, db::DatabaseContext};

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

	/// Get reference to the database context
	pub fn database(&self) -> &DatabaseContext {
		&self.database
	}
}
