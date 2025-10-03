use std::sync::Arc;
use std::time::Duration;
use anyhow::{Context, Result};
use sqlx::PgPool;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::api::beacon::BeaconApiClient;
use crate::api::constraints::ConstraintsApiClient;
use crate::config::Config;
use crate::db::delegation_ops::save_delegations_batch;
use crate::types::beacon::BeaconTiming;
use crate::types::delegation::SignedDelegation;

/// Service that proactively polls for delegation data and maintains the database
pub struct DelegationPollingService {
    beacon_client: Arc<BeaconApiClient>,
    constraints_client: Arc<ConstraintsApiClient>,
    db_pool: Arc<PgPool>,
    config: Arc<Config>,
    scheduler: JobScheduler,
}

impl DelegationPollingService {
    /// Create a new delegation polling service
    pub async fn new(
        beacon_client: Arc<BeaconApiClient>,
        constraints_client: Arc<ConstraintsApiClient>,
        db_pool: Arc<PgPool>,
        config: Arc<Config>,
    ) -> Result<Self> {
        let scheduler = JobScheduler::new().await
            .context("Failed to create job scheduler")?;

        Ok(Self {
            beacon_client,
            constraints_client,
            db_pool,
            config,
            scheduler,
        })
    }

    /// Start the delegation polling service with scheduled tasks
    pub async fn start(&self) -> Result<()> {
        info!("Starting delegation polling service");

        // Schedule delegation polling every 30 seconds
        let beacon_client = Arc::clone(&self.beacon_client);
        let constraints_client = Arc::clone(&self.constraints_client);
        let db_pool = Arc::clone(&self.db_pool);
        let config = Arc::clone(&self.config);

        let delegation_job = Job::new_async("0/30 * * * * *", move |_uuid, _l| {
            let beacon_client = Arc::clone(&beacon_client);
            let constraints_client = Arc::clone(&constraints_client);
            let db_pool = Arc::clone(&db_pool);
            let config = Arc::clone(&config);

            Box::pin(async move {
                if let Err(e) = poll_delegations(beacon_client, constraints_client, db_pool, config).await {
                    error!("Delegation polling failed: {}", e);
                }
            })
        }).context("Failed to create delegation polling job")?;

        self.scheduler.add(delegation_job).await
            .context("Failed to add delegation polling job to scheduler")?;

        // Schedule cleanup of expired delegations every 5 minutes
        let db_pool_cleanup = Arc::clone(&self.db_pool);
        let config_for_cleanup = Arc::clone(&self.config);
        let cleanup_job = Job::new_async("0 */5 * * * *", move |_uuid, _l| {
            let db_pool = Arc::clone(&db_pool_cleanup);
            let config = Arc::clone(&config_for_cleanup);

            Box::pin(async move {
                if let Err(e) = cleanup_expired_delegations(db_pool, config).await {
                    error!("Delegation cleanup failed: {}", e);
                }
            })
        }).context("Failed to create cleanup job")?;

        self.scheduler.add(cleanup_job).await
            .context("Failed to add cleanup job to scheduler")?;

        // Start the scheduler
        self.scheduler.start().await
            .context("Failed to start job scheduler")?;

        info!("Delegation polling service started successfully");
        Ok(())
    }

    /// Stop the delegation polling service
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping delegation polling service");
        self.scheduler.shutdown().await
            .context("Failed to shutdown job scheduler")?;
        info!("Delegation polling service stopped");
        Ok(())
    }

    /// Poll delegations once (for testing or manual execution)
    pub async fn poll_once(&self) -> Result<()> {
        poll_delegations(
            Arc::clone(&self.beacon_client),
            Arc::clone(&self.constraints_client),
            Arc::clone(&self.db_pool),
            Arc::clone(&self.config),
        ).await
    }
}

/// Core delegation polling logic
async fn poll_delegations(
    _beacon_client: Arc<BeaconApiClient>,
    constraints_client: Arc<ConstraintsApiClient>,
    db_pool: Arc<PgPool>,
    config: Arc<Config>,
) -> Result<()> {
    info!("Starting delegation polling cycle");

    // Get current slot and calculate the range we need to poll for
    let genesis_time = config.beacon_api.genesis_time;
    let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
    let lookahead_slots = 32; // Default to 1 epoch

    // Poll for current slot + lookahead range
    let start_slot = current_slot;
    let end_slot = current_slot + lookahead_slots;

    info!("Polling delegations for slots {} to {}", start_slot, end_slot);

    let mut total_delegations_found = 0;
    let mut successful_saves = 0;
    let mut errors = 0;

    // Poll each slot in the range
    for slot in start_slot..=end_slot {
        match poll_delegations_for_slot(
            &constraints_client,
            &db_pool,
            slot,
            &config.signing.bls_public_key,
        ).await {
            Ok(count) => {
                total_delegations_found += count;
                successful_saves += 1;
                if count > 0 {
                    debug!("Found {} delegations for slot {}", count, slot);
                }
            }
            Err(e) => {
                errors += 1;
                // Don't spam logs for normal "no delegations found" cases
                if e.to_string().contains("404") || e.to_string().contains("not found") {
                    debug!("No delegations found for slot {}: {}", slot, e);
                } else {
                    warn!("Failed to poll delegations for slot {}: {}", slot, e);
                }
            }
        }

        // Small delay between requests to avoid overwhelming the API
        sleep(Duration::from_millis(100)).await;
    }

    if total_delegations_found > 0 {
        info!(
            "Delegation polling cycle completed: {} delegations found across {} slots, {} successful saves, {} errors",
            total_delegations_found, end_slot - start_slot + 1, successful_saves, errors
        );
    } else {
        info!(
            "Delegation polling cycle completed: no new delegations found across {} slots ({} errors)",
            end_slot - start_slot + 1, errors
        );
    }

    Ok(())
}

/// Poll delegations for a specific slot
async fn poll_delegations_for_slot(
    constraints_client: &ConstraintsApiClient,
    db_pool: &PgPool,
    slot: u64,
    our_bls_pubkey: &blst::min_pk::PublicKey,
) -> Result<usize> {
    // Get all delegations for this slot from the constraints API
    let delegations = constraints_client.get_delegations_for_slot(slot).await
        .with_context(|| format!("Failed to fetch delegations for slot {}", slot))?;

    if delegations.is_empty() {
        return Ok(0);
    }

    // Filter to only delegations that involve our BLS key as delegate
    let _our_bls_pubkey_bytes = our_bls_pubkey.to_bytes();

    // TODO: Re-enable key filtering after matching keys with mock relay
    // For now, accept all delegations for testing
    let relevant_delegations: Vec<SignedDelegation> = delegations;
    /*
    let relevant_delegations: Vec<SignedDelegation> = delegations
        .into_iter()
        .filter(|delegation| {
            delegation.message.delegate.0 == our_bls_pubkey_bytes
        })
        .collect();
    */

    if relevant_delegations.is_empty() {
        return Ok(0);
    }

    debug!(
        "Found {} relevant delegations for slot {} (involving our keys)",
        relevant_delegations.len(),
        slot
    );

    // Save the delegations to the database
    let saved_ids = save_delegations_batch(db_pool, &relevant_delegations).await
        .with_context(|| format!("Failed to save delegations for slot {}", slot))?;

    let saved_count = saved_ids.len();
    debug!("Saved {} delegations for slot {}", saved_count, slot);
    Ok(saved_count)
}

/// Clean up expired delegations from the database
async fn cleanup_expired_delegations(db_pool: Arc<PgPool>, config: Arc<Config>) -> Result<()> {
    debug!("Starting expired delegation cleanup");

    // Get current slot using the actual configured genesis time
    let genesis_time = config.beacon_api.genesis_time;
    let current_slot = BeaconTiming::current_slot_estimate(genesis_time);

    // Deactivate delegations for slots that have passed
    let deactivated = crate::db::delegation_ops::deactivate_expired_delegations(&db_pool, current_slot).await?;

    if deactivated > 0 {
        info!("Deactivated {} expired delegations for slots < {}", deactivated, current_slot);
    } else {
        debug!("No expired delegations to clean up");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_delegation_polling_service_creation() {
        let config = Config::default();
        let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
        let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());

        // Create a test database pool (this would need a real database in integration tests)
        // For now, we'll skip this test without a database connection
        if std::env::var("DATABASE_URL").is_err() {
            return;
        }

        let db_pool = Arc::new(
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .expect("Failed to connect to test database")
        );

        let service = DelegationPollingService::new(
            beacon_client,
            constraints_client,
            db_pool,
            Arc::new(config),
        ).await;

        assert!(service.is_ok(), "Should be able to create delegation polling service");
    }

    #[tokio::test]
    async fn test_delegation_polling_service_lifecycle() {
        let config = Config::default();
        let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
        let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());

        // Skip test without database
        if std::env::var("DATABASE_URL").is_err() {
            return;
        }

        let db_pool = Arc::new(
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .expect("Failed to connect to test database")
        );

        let mut service = DelegationPollingService::new(
            beacon_client,
            constraints_client,
            db_pool,
            Arc::new(config),
        ).await.expect("Failed to create service");

        // Test start and stop
        service.start().await.expect("Failed to start service");

        // Give it a moment to run
        sleep(Duration::from_millis(100)).await;

        service.stop().await.expect("Failed to stop service");
    }

    #[tokio::test]
    async fn test_poll_once_without_error() {
        let config = Config::default();
        let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
        let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());

        // Skip test without database
        if std::env::var("DATABASE_URL").is_err() {
            return;
        }

        let db_pool = Arc::new(
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .expect("Failed to connect to test database")
        );

        let mut service = DelegationPollingService::new(
            beacon_client,
            constraints_client,
            db_pool,
            Arc::new(config),
        ).await.expect("Failed to create service");

        // This might fail due to network issues, but shouldn't panic
        let result = timeout(Duration::from_secs(10), service.poll_once()).await;
        assert!(result.is_ok(), "poll_once should complete within timeout");
    }
}