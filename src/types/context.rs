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
	pub beacon_client: Arc<BeaconApiClient>,
}

impl RpcContext {
	/// Creates a new RpcContext composed of the provided database context, configuration, fee pricing engine, and Beacon API client.
	///
	/// # Examples
	///
	/// ```
	/// use std::sync::Arc;
	/// // Construct or obtain the required components...
	/// let database = DatabaseContext::new();
	/// let config = Config::default();
	/// let fee_engine = Arc::new(FeePricingEngine::new());
	/// let beacon_client = Arc::new(BeaconApiClient::new());
	///
	/// let ctx = RpcContext::new(database, config, fee_engine, beacon_client);
	/// ```
	pub fn new(
		database: DatabaseContext,
		config: Config,
		fee_engine: Arc<FeePricingEngine>,
		beacon_client: Arc<BeaconApiClient>,
	) -> Self {
		Self { database, config, fee_engine, beacon_client }
	}

	/// Accesses the RpcContext's database context.
	///
	/// Provides a shared reference to the underlying DatabaseContext held by this RpcContext.
	///
	/// # Examples
	///
	/// ```
	/// // Construct RpcContext with appropriate values (placeholders shown)
	/// let ctx = RpcContext::new(db, config, fee_engine, beacon_client);
	/// let db_ref: &DatabaseContext = ctx.database();
	/// ```
	pub fn database(&self) -> &DatabaseContext {
		&self.database
	}

	/// Returns a reference to the internal Beacon API client.
	///
	/// # Returns
	///
	/// A reference to the `Arc<BeaconApiClient>` stored in the context.
	///
	/// # Examples
	///
	/// ```
	/// use std::sync::Arc;
	/// // assume db, config, fee_engine, beacon_client are available in scope
	/// let ctx = RpcContext::new(db, config, fee_engine, beacon_client.clone());
	/// let client_ref: &Arc<BeaconApiClient> = ctx.beacon_client();
	/// assert!(Arc::ptr_eq(client_ref, &beacon_client));
	/// ```
	pub fn beacon_client(&self) -> &Arc<BeaconApiClient> {
		&self.beacon_client
	}
}
