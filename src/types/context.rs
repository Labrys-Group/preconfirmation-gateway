use std::sync::Arc;
use crate::{config::Config, db::DatabaseContext};
use crate::services::fee_pricing::FeePricingEngine;

/// RPC context that provides access to shared resources for all RPC handlers
#[derive(Clone)]
pub struct RpcContext {
	/// Database context for PostgreSQL operations
	pub database: DatabaseContext,
	/// Configuration for validation and other settings
	pub config: Config,
	/// Fee pricing engine for dynamic fee calculation
	pub fee_engine: Arc<FeePricingEngine>,
}

impl RpcContext {
	/// Create a new RPC context with the given database context, config, and fee engine
	pub fn new(database: DatabaseContext, config: Config, fee_engine: Arc<FeePricingEngine>) -> Self {
		Self { database, config, fee_engine }
	}

	/// Get reference to the database context
	pub fn database(&self) -> &DatabaseContext {
		&self.database
	}
}
