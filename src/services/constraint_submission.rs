use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::{Context, Result};
use sqlx::PgPool;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::api::constraints::ConstraintsApiClient;
use crate::config::Config;
use crate::crypto::bls::BlsManager;
use crate::db::delegation_ops::get_delegations_for_slot;
use crate::types::beacon::BeaconTiming;
use crate::types::delegation::{ConstraintsMessage, Constraint, SignedConstraints, BlsSignature};

/// Service that handles timing-critical constraint submission to relays
pub struct ConstraintSubmissionService {
    constraints_client: Arc<ConstraintsApiClient>,
    bls_manager: Arc<BlsManager>,
    db_pool: Arc<PgPool>,
    config: Arc<Config>,
    scheduler: JobScheduler,
}

/// Represents a pending constraint submission
#[derive(Debug, Clone)]
pub struct PendingConstraint {
    pub slot: u64,
    pub payload: Vec<u8>,
    pub committer_address: String,
    pub submission_deadline: SystemTime,
}

impl ConstraintSubmissionService {
    /// Create a new constraint submission service
    pub async fn new(
        constraints_client: Arc<ConstraintsApiClient>,
        bls_manager: Arc<BlsManager>,
        db_pool: Arc<PgPool>,
        config: Arc<Config>,
    ) -> Result<Self> {
        let scheduler = JobScheduler::new().await
            .context("Failed to create constraint submission scheduler")?;

        Ok(Self {
            constraints_client,
            bls_manager,
            db_pool,
            config,
            scheduler,
        })
    }

    /// Start the constraint submission service
    pub async fn start(&self) -> Result<()> {
        info!("Starting constraint submission service");

        // Schedule constraint processing every 2 seconds for tight timing
        let constraints_client = Arc::clone(&self.constraints_client);
        let bls_manager = Arc::clone(&self.bls_manager);
        let db_pool = Arc::clone(&self.db_pool);
        let config = Arc::clone(&self.config);

        let submission_job = Job::new_async("*/2 * * * * *", move |_uuid, _l| {
            let constraints_client = Arc::clone(&constraints_client);
            let bls_manager = Arc::clone(&bls_manager);
            let db_pool = Arc::clone(&db_pool);
            let config = Arc::clone(&config);

            Box::pin(async move {
                if let Err(e) = process_pending_constraints(
                    constraints_client,
                    bls_manager,
                    db_pool,
                    config,
                ).await {
                    error!("Constraint submission processing failed: {}", e);
                }
            })
        }).context("Failed to create constraint submission job")?;

        self.scheduler.add(submission_job).await
            .context("Failed to add constraint submission job to scheduler")?;

        // Start the scheduler
        self.scheduler.start().await
            .context("Failed to start constraint submission scheduler")?;

        info!("Constraint submission service started successfully");
        Ok(())
    }

    /// Stop the constraint submission service
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping constraint submission service");
        self.scheduler.shutdown().await
            .context("Failed to shutdown constraint submission scheduler")?;
        info!("Constraint submission service stopped");
        Ok(())
    }

    /// Submit a single constraint immediately (for urgent/manual submission)
    pub async fn submit_constraint_now(
        &self,
        slot: u64,
        payload: Vec<u8>,
        committer_address: String,
    ) -> Result<String> {
        submit_constraint(
            &self.constraints_client,
            &self.bls_manager,
            &self.db_pool,
            &self.config,
            slot,
            payload,
            committer_address,
        ).await
    }

    /// Check if we're within the submission window for a given slot
    pub fn is_within_submission_window(&self, slot: u64) -> bool {
        let genesis_time = self.config.beacon_api.genesis_time;
        BeaconTiming::is_within_constraint_window(genesis_time, slot)
    }

    /// Calculate the submission deadline for a given slot
    pub fn get_submission_deadline(&self, slot: u64) -> SystemTime {
        let genesis_time = self.config.beacon_api.genesis_time;
        let deadline_timestamp = BeaconTiming::constraint_deadline_for_slot(genesis_time, slot);
        UNIX_EPOCH + Duration::from_secs(deadline_timestamp)
    }
}

/// Core constraint submission processing logic
async fn process_pending_constraints(
    constraints_client: Arc<ConstraintsApiClient>,
    bls_manager: Arc<BlsManager>,
    db_pool: Arc<PgPool>,
    config: Arc<Config>,
) -> Result<()> {
    debug!("Processing pending constraints");

    // Get current slot and determine which slots need constraint submission
    let genesis_time = config.beacon_api.genesis_time;
    let current_slot = BeaconTiming::current_slot_estimate(genesis_time);

    // Look ahead by a few slots to catch any pending constraints
    let lookahead = 3;
    let slots_to_check = (current_slot..=current_slot + lookahead).collect::<Vec<_>>();

    for slot in slots_to_check {
        // Check if we're still within the submission window
        if !BeaconTiming::is_within_constraint_window(genesis_time, slot) {
            debug!("Slot {} is outside the constraint submission window, skipping", slot);
            continue;
        }

        // Process constraints for this slot
        if let Err(e) = process_constraints_for_slot(
            &constraints_client,
            &bls_manager,
            &db_pool,
            &config,
            slot,
        ).await {
            warn!("Failed to process constraints for slot {}: {}", slot, e);
        }
    }

    Ok(())
}

/// Process constraints for a specific slot
async fn process_constraints_for_slot(
    constraints_client: &ConstraintsApiClient,
    bls_manager: &BlsManager,
    db_pool: &PgPool,
    config: &Config,
    slot: u64,
) -> Result<()> {
    // Get all delegations for this slot from our database
    let delegations = get_delegations_for_slot(db_pool, slot).await
        .with_context(|| format!("Failed to get delegations for slot {}", slot))?;

    if delegations.is_empty() {
        debug!("No delegations found for slot {}, skipping constraint processing", slot);
        return Ok(());
    }

    debug!("Processing constraints for slot {} with {} delegations", slot, delegations.len());

    // For each delegation, check if there are pending commitments that need constraint submission
    for delegation in delegations {
        // Check if this delegation matches our BLS public key
        if delegation.message.delegate.0 != config.signing.bls_public_key.to_bytes() {
            debug!(
                "Skipping delegation in slot {} - delegate key does not match our BLS public key",
                slot
            );
            continue;
        }

        // In a real implementation, we would query for pending commitments
        // that need to be converted to constraints for this slot/delegation
        // For now, we'll implement the framework and constraint creation logic

        if let Err(e) = process_delegation_constraints(
            constraints_client,
            bls_manager,
            db_pool,
            &delegation,
            &config.signing,
            slot,
        ).await {
            warn!(
                "Failed to process constraints for delegation in slot {}: {}",
                slot, e
            );
        }
    }

    Ok(())
}

/// Process constraints for a specific delegation
async fn process_delegation_constraints(
    constraints_client: &ConstraintsApiClient,
    bls_manager: &BlsManager,
    _db_pool: &PgPool,
    delegation: &crate::types::delegation::SignedDelegation,
    signing_config: &crate::config::SigningConfig,
    slot: u64,
) -> Result<()> {
    debug!(
        "Processing constraints for delegation in slot {} with committer {}",
        slot, delegation.message.committer
    );

    // Query the database for pending commitments that need constraint submission
    let constraints = create_constraints_from_commitments(_db_pool, slot).await?;

    if constraints.is_empty() {
        debug!("No constraints to submit for slot {}", slot);
        return Ok(());
    }

    // Create constraints message
    let constraints_message = ConstraintsMessage::new(
        delegation.message.proposer,
        delegation.message.delegate,
        slot,
        constraints,
        vec![], // receivers - empty for now
    );

    // Sign the constraints message with our BLS key
    let signature_bytes = bls_manager.sign_constraints_message(&constraints_message, &signing_config.bls_private_key)
        .context("Failed to sign constraints message")?;

    // Create SignedConstraints object
    let signed_constraints = SignedConstraints {
        message: constraints_message,
        signature: BlsSignature(signature_bytes),
    };

    // Submit to the constraints API
    let submission_response = constraints_client.submit_constraints(&signed_constraints)
        .await.context("Failed to submit constraints to API")?;

    info!(
        "Successfully submitted constraints for slot {} with response: {:?}",
        slot, submission_response
    );

    Ok(())
}

/// Submit a constraint immediately
async fn submit_constraint(
    constraints_client: &ConstraintsApiClient,
    bls_manager: &BlsManager,
    db_pool: &PgPool,
    config: &Config,
    slot: u64,
    payload: Vec<u8>,
    committer_address: String,
) -> Result<String> {
    // Verify the committer address matches our configured address
    if committer_address != config.signing.committer_address {
        return Err(anyhow::anyhow!(
            "Committer address {} does not match our configured address: {}",
            committer_address,
            config.signing.committer_address
        ));
    }

    // Get delegation for this slot
    let delegations = get_delegations_for_slot(db_pool, slot).await
        .context("Failed to get delegations for slot")?;

    let delegation = delegations
        .iter()
        .find(|d| d.message.delegate.0 == config.signing.bls_public_key.to_bytes()
                 && d.message.committer == committer_address)
        .ok_or_else(|| anyhow::anyhow!(
            "No valid delegation found for slot {} and committer {}", slot, committer_address
        ))?;

    // Create constraint from payload
    let constraint = Constraint::from_inclusion_commitment(payload);

    // Create constraints message
    let constraints_message = ConstraintsMessage::new(
        delegation.message.proposer,
        delegation.message.delegate,
        slot,
        vec![constraint],
        vec![], // receivers - empty for now
    );

    // Sign the constraints message
    let signature_bytes = bls_manager.sign_constraints_message(&constraints_message, &config.signing.bls_private_key)
        .context("Failed to sign constraints message")?;

    // Create SignedConstraints object
    let signed_constraints = SignedConstraints {
        message: constraints_message,
        signature: BlsSignature(signature_bytes),
    };

    // Submit with timeout to ensure we don't exceed the 8-second deadline
    let submission_result = timeout(
        Duration::from_secs(5), // Give ourselves 5 seconds to submit
        constraints_client.submit_constraints(&signed_constraints)
    ).await
    .context("Constraint submission timed out")?
    .context("Failed to submit constraint to API")?;

    info!(
        "Successfully submitted constraint for slot {} with response: {:?}",
        slot, submission_result
    );

    // For now, return a simple string representation - in real use, you might want to extract specific fields
    Ok(format!("{:?}", submission_result))
}

/// Create constraints from committed transactions for a specific slot
async fn create_constraints_from_commitments(
    db_pool: &PgPool,
    slot: u64,
) -> Result<Vec<Constraint>> {
    debug!("Creating constraints from commitments for slot {}", slot);

    // Query the database for unprocessed commitments for this slot
    let commitments = crate::db::operations::get_unprocessed_commitments_for_slot(db_pool, slot)
        .await
        .with_context(|| format!("Failed to get unprocessed commitments for slot {}", slot))?;

    if commitments.is_empty() {
        debug!("No unprocessed commitments found for slot {}", slot);
        return Ok(vec![]);
    }

    debug!("Found {} unprocessed commitments for slot {}", commitments.len(), slot);

    // Convert commitments to constraints
    let constraints: Vec<Constraint> = commitments
        .iter()
        .filter_map(|signed_commitment| {
            let commitment = &signed_commitment.commitment;

            // Only process inclusion commitments (type 1)
            if commitment.commitment_type == 1 {
                Some(Constraint::from_inclusion_commitment(commitment.payload.clone()))
            } else {
                warn!(
                    "Skipping commitment with unsupported type {} for slot {}",
                    commitment.commitment_type, slot
                );
                None
            }
        })
        .collect();

    // Mark the commitments as processed to prevent duplicate submissions
    let request_hashes: Vec<String> = commitments
        .iter()
        .map(|sc| sc.commitment.request_hash.clone())
        .collect();

    if !request_hashes.is_empty() {
        let marked = crate::db::operations::mark_commitments_as_processed(db_pool, &request_hashes)
            .await
            .context("Failed to mark commitments as processed")?;

        debug!("Marked {} commitments as processed for slot {}", marked, slot);
    }

    info!("Created {} constraints from commitments for slot {}", constraints.len(), slot);
    Ok(constraints)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::time::Duration;

    #[tokio::test]
    async fn test_constraint_submission_service_creation() {
        let config = Config::default();
        let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
        let bls_manager = Arc::new(BlsManager::new("0x00000000").expect("Failed to create BLS manager"));

        // Skip test without database
        if std::env::var("DATABASE_URL").is_err() {
            return;
        }

        let db_pool = Arc::new(
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .expect("Failed to connect to test database")
        );

        let service = ConstraintSubmissionService::new(
            constraints_client,
            bls_manager,
            db_pool,
            Arc::new(config),
        ).await;

        assert!(service.is_ok(), "Should be able to create constraint submission service");
    }

    #[tokio::test]
    async fn test_submission_window_check() {
        let config = Config::default();
        let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
        let bls_manager = Arc::new(BlsManager::new("0x00000000").expect("Failed to create BLS manager"));

        // Skip test without database
        if std::env::var("DATABASE_URL").is_err() {
            return;
        }

        let db_pool = Arc::new(
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .expect("Failed to connect to test database")
        );

        let service = ConstraintSubmissionService::new(
            constraints_client,
            bls_manager,
            db_pool,
            Arc::new(config),
        ).await.expect("Failed to create service");

        // Test with a future slot (should be within window)
        let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let future_slot = (current_time / 12) + 10; // 10 slots in the future

        // Note: This test will depend on the actual beacon chain timing
        // In a real environment, you might want to mock the timing functions
        let deadline = service.get_submission_deadline(future_slot);
        assert!(deadline > SystemTime::now(), "Future slot deadline should be in the future");
    }

    #[tokio::test]
    async fn test_service_lifecycle() {
        let config = Config::default();
        let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
        let bls_manager = Arc::new(BlsManager::new("0x00000000").expect("Failed to create BLS manager"));

        // Skip test without database
        if std::env::var("DATABASE_URL").is_err() {
            return;
        }

        let db_pool = Arc::new(
            sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
                .await
                .expect("Failed to connect to test database")
        );

        let mut service = ConstraintSubmissionService::new(
            constraints_client,
            bls_manager,
            db_pool,
            Arc::new(config),
        ).await.expect("Failed to create service");

        // Test start and stop
        service.start().await.expect("Failed to start service");

        // Give it a moment to run
        tokio::time::sleep(Duration::from_millis(100)).await;

        service.stop().await.expect("Failed to stop service");
    }
}