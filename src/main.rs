use preconfirmation_gateway::{api, config, crypto, db, server, services, types};

/// Start the application: initialize configuration, services, background tasks, and run the RPC server until shutdown.
///
/// This function performs full startup of the service: it loads configuration, configures logging, initializes the
/// database and API clients, starts background services (fee pricing cache refresh, delegation polling, constraint
/// submission), spawns metrics collection and an HTTP metrics endpoint, and finally runs the RPC server. On shutdown
/// it aborts background tasks and exits.
///
/// # Returns
///
/// `Ok(())` on successful startup and clean shutdown, or an `Err` containing the error that prevented startup.
///
/// # Examples
///
/// ```no_run
/// // Start the application (runs until shutdown)
/// # use anyhow::Result;
/// # async fn run() -> Result<()> {
/// crate::main().await?;
/// # Ok(())
/// # }
/// ```
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
	let reth_client = std::sync::Arc::new(api::reth::RethApiClient::new(reth_api_config)?);

	// Initialize fee pricing engine
	let fee_engine = std::sync::Arc::new(services::fee_pricing::FeePricingEngine::new(
		reth_client,
		std::sync::Arc::new(db_context.clone()),
		std::sync::Arc::new(config.clone()),
	));

	// Start fee pricing cache refresh service
	fee_engine.start_cache_refresh_service().await?;
	tracing::info!("Fee pricing cache refresh service started");

	// Initialize beacon API client (needed for RPC context and background services)
	let beacon_client = std::sync::Arc::new(api::beacon::BeaconApiClient::new(config.beacon_api.clone())?);

	// Create RPC context with database context, config, fee engine, and beacon client
	let rpc_context =
		types::RpcContext::new(db_context.clone(), config.clone(), fee_engine.clone(), beacon_client.clone());
	let constraints_client =
		std::sync::Arc::new(api::constraints::ConstraintsApiClient::new(config.constraints_api.clone())?);

	// Start delegation polling service
	let delegation_service = services::delegation_polling::DelegationPollingService::new(
		beacon_client.clone(),
		constraints_client.clone(),
		std::sync::Arc::new(db_context.pool().clone()),
		std::sync::Arc::new(config.clone()),
	)
	.await?;

	delegation_service.start().await?;
	tracing::info!("Delegation polling service started");

	// Trigger an immediate poll to fetch delegations on startup
	if let Err(e) = delegation_service.poll_once().await {
		tracing::warn!("Initial delegation poll failed: {}", e);
	}

	// Initialize BLS Manager for constraint signing
	let bls_manager = std::sync::Arc::new(crypto::bls::BlsManager::new(&config.delegation.domain_application_gateway)?);

	// Start constraint submission service
	let constraint_service = services::constraint_submission::ConstraintSubmissionService::new(
		constraints_client.clone(),
		bls_manager,
		std::sync::Arc::new(db_context.pool().clone()),
		std::sync::Arc::new(config.clone()),
	)
	.await?;

	constraint_service.start().await?;
	tracing::info!("Constraint submission service started");

	// Initialize Prometheus metrics
	let metrics_registry = std::sync::Arc::new(preconfirmation_gateway::metrics::MetricsRegistry::new()?);
	tracing::info!("Prometheus metrics registry initialized");

	// Start background metrics updater
	let metrics_for_updater = metrics_registry.clone();
	let db_for_metrics = db_context.clone();
	let fee_for_metrics = fee_engine.clone();

	let metrics_updater = tokio::spawn(async move {
		use tokio::time::{Duration, interval};
		let mut ticker = interval(Duration::from_secs(15));

		loop {
			ticker.tick().await;
			// Update metrics directly here to avoid type issues
			if let Ok(commitment_stats) = db_for_metrics.get_stats().await {
				metrics_for_updater
					.update_commitment_stats(commitment_stats.total_count, commitment_stats.commitment_type_1_count);
			}
			if let Ok(delegation_stats) = db_for_metrics.get_delegation_stats().await {
				metrics_for_updater.update_delegation_stats(
					delegation_stats.total_count,
					delegation_stats.active_count,
					delegation_stats.unique_proposers,
					delegation_stats.unique_delegates,
					delegation_stats.slots_covered,
				);
			}
			if let Ok(congestion_stats) = db_for_metrics.get_congestion_stats().await {
				metrics_for_updater.update_congestion_stats(
					congestion_stats.current_average_congestion,
					congestion_stats.highest_congestion_ratio,
					congestion_stats.average_fee_multiplier,
				);
			}
			if let Ok(pricing_stats) = fee_for_metrics.get_pricing_stats().await {
				metrics_for_updater
					.update_pricing_stats(pricing_stats.current_slot, pricing_stats.current_base_gas_price);
			}
		}
	});
	tracing::info!("Metrics updater started");

	// Start metrics HTTP server on port 9090
	let metrics_server = {
		let metrics_registry = metrics_registry.clone();
		tokio::spawn(async move {
			use hyper::service::{make_service_fn, service_fn};
			use hyper::{Body, Request, Response, Server};
			use std::convert::Infallible;

			/// Serves Prometheus-formatted metrics from the given registry.
			///
			/// Attempts to render metrics from `metrics` and returns an HTTP 200 response with
			/// `text/plain; version=0.0.4` on success or a 500 response with "Internal Server Error"
			/// if rendering fails. The incoming request is ignored.
			///
			/// # Examples
			///
			/// ```
			/// use hyper::{Body, Request, StatusCode};
			/// use preconfirmation_gateway::metrics::MetricsRegistry;
			/// # tokio_test::block_on(async {
			/// let registry = std::sync::Arc::new(MetricsRegistry::new());
			/// let req = Request::new(Body::empty());
			/// let resp = super::metrics_handler(registry, req).await.unwrap();
			/// assert_eq!(resp.status(), StatusCode::OK);
			/// # });
			/// ```
			async fn metrics_handler(
				metrics: std::sync::Arc<preconfirmation_gateway::metrics::MetricsRegistry>,
				_req: Request<Body>,
			) -> Result<Response<Body>, Infallible> {
				match metrics.render_metrics() {
					Ok(body) => Ok(Response::builder()
						.status(200)
						.header("Content-Type", "text/plain; version=0.0.4")
						.body(Body::from(body))
						.unwrap()),
					Err(e) => {
						tracing::error!("Failed to render metrics: {}", e);
						Ok(Response::builder().status(500).body(Body::from("Internal Server Error")).unwrap())
					}
				}
			}

			let addr = ([0, 0, 0, 0], 9090).into();

			let make_svc = make_service_fn(move |_conn| {
				let metrics = metrics_registry.clone();
				async move { Ok::<_, Infallible>(service_fn(move |req| metrics_handler(metrics.clone(), req))) }
			});

			let server = Server::bind(&addr).serve(make_svc);
            tracing::info!("Metrics server listening on http://{}/metrics", addr);

			if let Err(e) = server.await {
				tracing::error!("Metrics server error: {}", e);
			}
		})
	};

	// Run the RPC server (this blocks until shutdown)
	server::run_server(rpc_context, &config).await?;

	// Cleanup on shutdown
	metrics_server.abort();

	// Cleanup on shutdown
	metrics_updater.abort();

	Ok(())
}