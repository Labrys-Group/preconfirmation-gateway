use crate::api::beacon::BeaconApiClient;
use crate::services::fee_pricing::FeePricingEngine;
use crate::{config::Config, db::DatabaseContext};
use std::sync::Arc;

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
	pub beacon_client: Arc<BeaconApiClient<crate::api::beacon::ReqwestClient>>,
}

impl RpcContext {
	/// Creates a new RpcContext composed of the provided database context, configuration, fee pricing engine, and Beacon API client.
	///
	/// # Examples
	///
	pub fn new(
		database: DatabaseContext,
		config: Config,
		fee_engine: Arc<FeePricingEngine>,
		beacon_client: Arc<BeaconApiClient<crate::api::beacon::ReqwestClient>>,
	) -> Self {
		Self { database, config, fee_engine, beacon_client }
	}

	/// Accesses the RpcContext's database context.
	///
	/// Provides a shared reference to the underlying DatabaseContext held by this RpcContext.
	///
	/// # Examples
	///
	pub fn database(&self) -> &DatabaseContext {
		&self.database
	}

	/// Returns a reference to the internal Beacon API client.
	///
	/// # Returns
	///
	/// A reference to the `Arc<BeaconApiClient<crate::api::beacon::ReqwestClient>>` stored in the context.
	///
	/// # Examples
	///
	pub fn beacon_client(&self) -> &Arc<BeaconApiClient<crate::api::beacon::ReqwestClient>> {
		&self.beacon_client
	}
}
