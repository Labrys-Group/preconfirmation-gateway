use std::sync::Arc;
use crate::{config::Config, db::DatabaseContext};
use crate::services::fee_pricing::FeePricingEngine;
use crate::api::beacon::BeaconApiClient;

/// RPC context that provides access to shared resources for all RPC handlers
#[derive(Clone)]
pub struct RpcContext {
	/// Database context for PostgreSQL operations
	pub database: DatabaseContext,
	/// Configuration for validation and other settings
	pub config: Config,
	/// Fee pricing engine for dynamic fee calculation
	pub fee_engine: Arc<FeePricingEngine>,
	/// Beacon API client for validator duty verification
	pub beacon_client: Arc<BeaconApiClient>,
}

impl RpcContext {
	/// Create a new RPC context with the given database context, config, fee engine, and beacon client
	pub fn new(
		database: DatabaseContext,
		config: Config,
		fee_engine: Arc<FeePricingEngine>,
		beacon_client: Arc<BeaconApiClient>,
	) -> Self {
		Self { database, config, fee_engine, beacon_client }
	}

	/// Get reference to the database context
	pub fn database(&self) -> &DatabaseContext {
		&self.database
	}

	/// Get reference to the beacon API client
	pub fn beacon_client(&self) -> &Arc<BeaconApiClient> {
		&self.beacon_client
	}
}
