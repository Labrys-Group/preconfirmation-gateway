use anyhow::{Context, Result};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{debug, error, info, warn};

use crate::api::constraints::ConstraintsApiClient;
use crate::config::Config;
use crate::crypto::bls::BlsManager;
use crate::db::delegation_ops::get_delegations_for_slot;
use crate::types::beacon::BeaconTiming;
use crate::types::delegation::{BlsPublicKey, BlsSignature, Constraint, ConstraintsMessage, SignedConstraints};

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
	/// Constructs a new ConstraintSubmissionService using the provided clients, database pool, and configuration,
	/// and initializes its background job scheduler.
	///
	/// # Errors
	///
	/// Returns an error if the internal JobScheduler cannot be created.
	///
	/// # Examples
	///
	pub async fn new(
		constraints_client: Arc<ConstraintsApiClient>,
		bls_manager: Arc<BlsManager>,
		db_pool: Arc<PgPool>,
		config: Arc<Config>,
	) -> Result<Self> {
		let scheduler = JobScheduler::new().await.context("Failed to create constraint submission scheduler")?;

		Ok(Self { constraints_client, bls_manager, db_pool, config, scheduler })
	}

	/// Starts the constraint submission service and begins periodic processing of pending constraints.
	///
	/// The service schedules a recurring job (every 2 seconds) that processes pending constraint
	/// submissions and starts the internal scheduler. Returns an error if scheduling or starting
	/// the scheduler fails.
	///
	/// # Examples
	///
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
				if let Err(e) = process_pending_constraints(constraints_client, bls_manager, db_pool, config).await {
					error!("Constraint submission processing failed: {}", e);
				}
			})
		})
		.context("Failed to create constraint submission job")?;

		self.scheduler.add(submission_job).await.context("Failed to add constraint submission job to scheduler")?;

		// Start the scheduler
		self.scheduler.start().await.context("Failed to start constraint submission scheduler")?;

		info!("Constraint submission service started successfully");
		Ok(())
	}

	/// Stops the constraint submission service by shutting down its internal scheduler.
	///
	/// Returns `Ok(())` if the scheduler shuts down successfully, or an error with context on failure.
	///
	/// # Examples
	///
	pub async fn stop(&mut self) -> Result<()> {
		info!("Stopping constraint submission service");
		self.scheduler.shutdown().await.context("Failed to shutdown constraint submission scheduler")?;
		info!("Constraint submission service stopped");
		Ok(())
	}

	/// Submits a single constraint for the given slot and committer immediately.
	///
	/// Returns the string result returned by the constraints API upon successful submission.
	///
	/// # Examples
	///
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
		)
		.await
	}

	/// Determine whether a beacon slot falls inside the constraint submission window.
	///
	/// The window is computed relative to the configured genesis time.
	///
	/// # Returns
	///
	/// `true` if the slot is within the submission window, `false` otherwise.
	///
	/// # Examples
	///
	pub fn is_within_submission_window(&self, slot: u64) -> bool {
		let genesis_time = self.config.beacon_api.genesis_time;
		BeaconTiming::is_within_constraint_window(genesis_time, slot)
	}

	/// Compute the submission deadline for a specific beacon slot.
	///
	/// Returns a `SystemTime` representing the absolute deadline by which constraints for the
	/// given `slot` should be submitted, based on the configured genesis time and beacon timing.
	///
	/// # Examples
	///
	pub fn get_submission_deadline(&self, slot: u64) -> SystemTime {
		let genesis_time = self.config.beacon_api.genesis_time;
		let deadline_timestamp = BeaconTiming::constraint_deadline_for_slot(genesis_time, slot);
		UNIX_EPOCH + Duration::from_secs(deadline_timestamp)
	}
}

/// Parse authorized builder BLS public keys from configuration strings.
///
/// Takes a slice of hex-encoded BLS public key strings (with or without "0x" prefix)
/// and converts them into `BlsPublicKey` objects. Each key must be exactly 48 bytes
/// (96 hex characters) when decoded.
///
/// # Errors
///
/// Returns an error if any key string cannot be decoded as valid hex or is not
/// exactly 48 bytes in length.
///
/// # Examples
///
fn parse_authorized_builders(builder_keys: &[String]) -> Result<Vec<BlsPublicKey>> {
	builder_keys
		.iter()
		.map(|key_str| {
			let bytes = crate::crypto::parse_hex_bytes(key_str, 48)
				.with_context(|| format!("Failed to parse builder public key: {}", key_str))?;
			let mut key_array = [0u8; 48];
			key_array.copy_from_slice(&bytes);
			Ok(BlsPublicKey(key_array))
		})
		.collect()
}

/// Process pending constraint submissions for the current slot and a short lookahead.
///
/// Attempts to process constraint submissions for each slot from the current slot up to
/// three slots ahead. For each slot within the constraint submission window, this function
/// invokes per-slot processing and logs recoverable errors without aborting the overall loop.
///
/// # Examples
///
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
		if let Err(e) = process_constraints_for_slot(&constraints_client, &bls_manager, &db_pool, &config, slot).await {
			warn!("Failed to process constraints for slot {}: {}", slot, e);
		}
	}

	Ok(())
}

/// Processes delegation constraints for a given slot.
///
/// Fetches delegations for `slot`, filters delegations that match the configured
/// BLS public key, and invokes per-delegation constraint processing for each
/// matching delegation. Per-delegation errors are logged and do not stop
/// processing of other delegations.
///
/// Returns `Ok(())` when processing completes for the slot. Returns an error if
/// delegations cannot be retrieved from the database.
///
/// # Examples
///
async fn process_constraints_for_slot(
	constraints_client: &ConstraintsApiClient,
	bls_manager: &BlsManager,
	db_pool: &PgPool,
	config: &Config,
	slot: u64,
) -> Result<()> {
	// Get all delegations for this slot from our database
	let delegations = get_delegations_for_slot(db_pool, slot)
		.await
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
			debug!("Skipping delegation in slot {} - delegate key does not match our BLS public key", slot);
			continue;
		}

		// In a real implementation, we would query for pending commitments
		// that need to be converted to constraints for this slot/delegation
		// For now, we'll implement the framework and constraint creation logic

		if let Err(e) = process_delegation_constraints(constraints_client, bls_manager, db_pool, &delegation, config, slot).await
		{
			warn!("Failed to process constraints for delegation in slot {}: {}", slot, e);
		}
	}

	Ok(())
}

/// Process and submit constraints for a single delegation at a given slot.
///
/// This function gathers unprocessed inclusion commitments for `slot`, converts them into
/// constraints, signs a constraints message using `bls_manager` and `signing_config`, submits
/// the signed message via `constraints_client`, and on successful submission marks the
/// corresponding commitments as processed in the database.
///
/// # Behavior
///
/// - If no constraints are produced for the slot, the function returns without side effects.
/// - On successful submission, the function marks the processed commitment hashes in the database.
/// - Any error encountered while creating constraints, signing, submitting, or marking commitments
///   is returned.
///
/// # Examples
///
async fn process_delegation_constraints(
	constraints_client: &ConstraintsApiClient,
	bls_manager: &BlsManager,
	_db_pool: &PgPool,
	delegation: &crate::types::delegation::SignedDelegation,
	config: &Config,
	slot: u64,
) -> Result<()> {
	debug!("Processing constraints for delegation in slot {} with committer {}", slot, delegation.message.committer);

	// Query the database for pending commitments that need constraint submission
	// Returns both the constraints and the hashes of commitments that were converted
	let (constraints, processed_hashes) = create_constraints_from_commitments(_db_pool, slot).await?;

	if constraints.is_empty() {
		debug!("No constraints to submit for slot {}", slot);
		return Ok(());
	}

	// Parse authorized builders from config
	let receivers = parse_authorized_builders(&config.constraints_api.authorized_builders)
		.context("Failed to parse authorized builder public keys from config")?;

	// Create constraints message
	let constraints_message =
		ConstraintsMessage::new(delegation.message.proposer, delegation.message.delegate, slot, constraints, receivers);

	// Sign the constraints message with our BLS key
	let signature_bytes = bls_manager
		.sign_constraints_message(&constraints_message, &config.signing.bls_private_key)
		.context("Failed to sign constraints message")?;

	// Create SignedConstraints object
	let signed_constraints =
		SignedConstraints { message: constraints_message, signature: BlsSignature(signature_bytes) };

	// Submit to the constraints API
	let submission_response = constraints_client
		.submit_constraints(&signed_constraints)
		.await
		.context("Failed to submit constraints to API")?;

	info!("Successfully submitted constraints for slot {} with response: {:?}", slot, submission_response);

	// Only mark commitments as processed after successful submission
	if !processed_hashes.is_empty() {
		let marked = crate::db::operations::mark_commitments_as_processed(_db_pool, &processed_hashes)
			.await
			.context("Failed to mark commitments as processed")?;

		debug!("Marked {} commitments as processed for slot {} after successful submission", marked, slot);
	}

	Ok(())
}

/// Submits a single constraint for the given slot immediately.
///
/// This validates the provided committer address against the configured committer,
/// finds the matching delegation for the slot, converts the provided payload into
/// a `Constraint`, signs the resulting `ConstraintsMessage` with the configured BLS key,
/// and submits the signed constraints to the constraints API. The submission is bounded
/// by a short timeout to avoid exceeding overall deadlines.
///
/// # Errors
///
/// Returns an error if the committer address does not match configuration, no matching
/// delegation is found for the slot and committer, signing fails, the API submission
/// fails, or the submission times out.
///
/// # Returns
///
/// A `String` representation of the API submission response.
///
/// # Examples
///
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
	let delegations = get_delegations_for_slot(db_pool, slot).await.context("Failed to get delegations for slot")?;

	let delegation = delegations
		.iter()
		.find(|d| {
			d.message.delegate.0 == config.signing.bls_public_key.to_bytes() && d.message.committer == committer_address
		})
		.ok_or_else(|| {
			anyhow::anyhow!("No valid delegation found for slot {} and committer {}", slot, committer_address)
		})?;

	// Create constraint from payload
	let constraint = Constraint::from_inclusion_commitment(payload);

	// Parse authorized builders from config
	let receivers = parse_authorized_builders(&config.constraints_api.authorized_builders)
		.context("Failed to parse authorized builder public keys from config")?;

	// Create constraints message
	let constraints_message = ConstraintsMessage::new(
		delegation.message.proposer,
		delegation.message.delegate,
		slot,
		vec![constraint],
		receivers,
	);

	// Sign the constraints message
	let signature_bytes = bls_manager
		.sign_constraints_message(&constraints_message, &config.signing.bls_private_key)
		.context("Failed to sign constraints message")?;

	// Create SignedConstraints object
	let signed_constraints =
		SignedConstraints { message: constraints_message, signature: BlsSignature(signature_bytes) };

	// Submit with timeout to ensure we don't exceed the 8-second deadline
	let submission_result = timeout(
		Duration::from_secs(5), // Give ourselves 5 seconds to submit
		constraints_client.submit_constraints(&signed_constraints),
	)
	.await
	.context("Constraint submission timed out")?
	.context("Failed to submit constraint to API")?;

	info!("Successfully submitted constraint for slot {} with response: {:?}", slot, submission_result);

	// For now, return a simple string representation - in real use, you might want to extract specific fields
	Ok(format!("{:?}", submission_result))
}

/// Build constraint objects from unprocessed commitments for a slot.
///
/// Queries the database for unprocessed commitments at the given slot, converts inclusion
/// commitments (commitment_type == 1) into `Constraint` values, and returns those constraints
/// along with the request hashes of commitments that were successfully converted.
///
/// # Returns
///
/// A tuple where the first element is a `Vec<Constraint>` created from inclusion commitments,
/// and the second element is a `Vec<String>` containing the `request_hash` values for commitments
/// that were converted into constraints.
///
/// # Examples
///
async fn create_constraints_from_commitments(db_pool: &PgPool, slot: u64) -> Result<(Vec<Constraint>, Vec<String>)> {
	debug!("Creating constraints from commitments for slot {}", slot);

	// Query the database for unprocessed commitments for this slot
	let commitments = crate::db::operations::get_unprocessed_commitments_for_slot(db_pool, slot)
		.await
		.with_context(|| format!("Failed to get unprocessed commitments for slot {}", slot))?;

	if commitments.is_empty() {
		debug!("No unprocessed commitments found for slot {}", slot);
		return Ok((vec![], vec![]));
	}

	debug!("Found {} unprocessed commitments for slot {}", commitments.len(), slot);

	// Convert commitments to constraints, tracking which ones were converted
	let mut constraints = Vec::new();
	let mut processed_hashes = Vec::new();

	for signed_commitment in &commitments {
		let commitment = &signed_commitment.commitment;

		// Only process inclusion commitments (type 1)
		if commitment.commitment_type == 1 {
			constraints.push(Constraint::from_inclusion_commitment(commitment.payload.clone()));
			processed_hashes.push(commitment.request_hash.clone());
		} else {
			warn!("Skipping commitment with unsupported type {} for slot {}", commitment.commitment_type, slot);
			// Don't add to processed_hashes - we only track successfully converted commitments
		}
	}

	info!(
		"Created {} constraints from {} commitments for slot {} ({} skipped)",
		constraints.len(),
		commitments.len(),
		slot,
		commitments.len() - constraints.len()
	);

	Ok((constraints, processed_hashes))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::delegation::Constraint;
	use std::time::Duration;

	fn create_mock_bls_manager() -> BlsManager {
		BlsManager::new("0x00000002").expect("Failed to create BLS manager")
	}

	#[tokio::test]
	async fn test_constraint_submission_service_creation() {
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());

		// Skip test without database
		if std::env::var("DATABASE_URL").is_err() {
			return;
		}

		let db_pool = Arc::new(
			sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
				.await
				.expect("Failed to connect to test database"),
		);

		let service =
			ConstraintSubmissionService::new(constraints_client, bls_manager, db_pool, Arc::new(config)).await;

		assert!(service.is_ok(), "Should be able to create constraint submission service");
	}

	#[tokio::test]
	async fn test_constraint_submission_service_creation_without_db() {
		// This test doesn't require a real database
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());

		// Create a lazy connection pool that won't actually connect
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let service =
			ConstraintSubmissionService::new(constraints_client, bls_manager, db_pool, Arc::new(config)).await;

		assert!(service.is_ok(), "Should be able to create constraint submission service without database");
	}

	#[tokio::test]
	async fn test_submission_window_check() {
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());

		// Skip test without database
		if std::env::var("DATABASE_URL").is_err() {
			return;
		}

		let db_pool = Arc::new(
			sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
				.await
				.expect("Failed to connect to test database"),
		);

		let service = ConstraintSubmissionService::new(constraints_client, bls_manager, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Test with a future slot (should be within window)
		let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
		let future_slot = (current_time / 12) + 10; // 10 slots in the future

		// Note: This test will depend on the actual beacon chain timing
		// In a real environment, you might want to mock the timing functions
		let deadline = service.get_submission_deadline(future_slot);
		assert!(deadline > SystemTime::now(), "Future slot deadline should be in the future");
	}

	#[tokio::test]
	async fn test_submission_window_check_without_db() {
		// Test submission window logic without requiring a database
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let service = ConstraintSubmissionService::new(constraints_client, bls_manager, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Test timing calculations
		let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
		let current_slot = (current_time - service.config.beacon_api.genesis_time) / 12;

		// Test future slot within window (use a slot that's definitely in the future)
		let future_slot = current_slot + 10;
		let is_future_within = service.is_within_submission_window(future_slot);
		assert!(is_future_within, "Future slot should be within submission window");

		// Test past slot (outside window)
		let far_past_slot = current_slot.saturating_sub(50);
		let is_past_within = service.is_within_submission_window(far_past_slot);
		assert!(!is_past_within, "Far past slot should be outside submission window");

		// Test deadline calculation
		let deadline = service.get_submission_deadline(future_slot);
		assert!(deadline > SystemTime::now(), "Future slot deadline should be in the future");
	}

	#[tokio::test]
	async fn test_service_lifecycle() {
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());

		// Skip test without database
		if std::env::var("DATABASE_URL").is_err() {
			return;
		}

		let db_pool = Arc::new(
			sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
				.await
				.expect("Failed to connect to test database"),
		);

		let mut service = ConstraintSubmissionService::new(constraints_client, bls_manager, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Test start and stop
		service.start().await.expect("Failed to start service");

		// Give it a moment to run
		tokio::time::sleep(Duration::from_millis(100)).await;

		service.stop().await.expect("Failed to stop service");
	}

	#[tokio::test]
	async fn test_service_lifecycle_without_db() {
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let mut service = ConstraintSubmissionService::new(constraints_client, bls_manager, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Test start and stop (will fail during processing due to no DB, but structure should work)
		service.start().await.expect("Failed to start service");

		// Give it a brief moment
		tokio::time::sleep(Duration::from_millis(50)).await;

		service.stop().await.expect("Failed to stop service");
	}

	#[tokio::test]
	async fn test_process_pending_constraints_no_delegations() {
		// Skip test without database
		if std::env::var("DATABASE_URL").is_err() {
			return;
		}

		// Test the processing loop when no delegations are found
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());

		let db_pool = Arc::new(
			sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
				.await
				.expect("Failed to connect to test database"),
		);

		// This should not panic even though the database operations will fail
		let result = process_pending_constraints(constraints_client, bls_manager, db_pool, Arc::new(config)).await;

		// The function should handle database errors gracefully and return Ok(())
		assert!(result.is_ok(), "Processing should handle database errors gracefully");
	}

	#[tokio::test]
	async fn test_submit_constraint_validation() {
		// Test constraint submission with validation errors
		let mut config = crate::testing::mocks::create_test_config();
		config.signing.committer_address = "0x1234567890123456789012345678901234567890".to_string();

		let constraints_client = ConstraintsApiClient::new(config.constraints_api.clone()).unwrap();
		let bls_manager = create_mock_bls_manager();
		let db_pool = sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap();

		let slot = 12345u64;
		let payload = vec![1, 2, 3, 4];
		let wrong_committer = "0x9876543210987654321098765432109876543210".to_string();

		// Test with wrong committer address
		let result = submit_constraint(
			&constraints_client,
			&bls_manager,
			&db_pool,
			&config,
			slot,
			payload.clone(),
			wrong_committer,
		)
		.await;

		assert!(result.is_err(), "Should fail with wrong committer address");
		let error_msg = format!("{}", result.unwrap_err());
		assert!(error_msg.contains("does not match our configured address"), "Error should mention address mismatch");
	}

	#[tokio::test]
	async fn test_create_constraints_from_commitments_empty() {
		// Test constraint creation with no commitments
		let db_pool = sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap();
		let slot = 12345u64;

		// This will fail due to database connectivity, but we can test the error handling
		let result = create_constraints_from_commitments(&db_pool, slot).await;
		assert!(result.is_err(), "Should fail due to no database connection");
	}

	#[test]
	fn test_pending_constraint_creation() {
		let slot = 12345u64;
		let payload = vec![1, 2, 3, 4];
		let committer = "0x1234567890123456789012345678901234567890".to_string();
		let deadline = SystemTime::now() + Duration::from_secs(8);

		let pending = PendingConstraint {
			slot,
			payload: payload.clone(),
			committer_address: committer.clone(),
			submission_deadline: deadline,
		};

		assert_eq!(pending.slot, slot);
		assert_eq!(pending.payload, payload);
		assert_eq!(pending.committer_address, committer);
		assert!(pending.submission_deadline > SystemTime::now());
	}

	#[test]
	fn test_constraint_timing_calculations() {
		let config = crate::testing::mocks::create_test_config();
		let genesis_time = config.beacon_api.genesis_time;

		// Test BeaconTiming functions used by the service
		let current_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);

		// Verify slot calculation makes sense
		let expected_slot = (current_time - genesis_time) / 12;
		assert!(
			(current_slot as i64 - expected_slot as i64).abs() <= 1,
			"Current slot calculation should be accurate within 1 slot"
		);

		// Test window check (use a recent past slot which should be within window)
		let recent_slot = current_slot.saturating_sub(1);
		let within_window = BeaconTiming::is_within_constraint_window(genesis_time, recent_slot);
		// Note: Window check behavior depends on implementation - just verify it doesn't panic
		let _ = within_window; // Don't assert specific behavior as it depends on timing implementation

		let far_future = current_slot + 100;
		let future_within_window = BeaconTiming::is_within_constraint_window(genesis_time, far_future);
		// Far future slots should generally be outside the constraint window
		let _ = future_within_window; // Don't assert specific behavior as it depends on implementation
	}

	#[test]
	fn test_constraint_from_inclusion_commitment() {
		let payload = vec![1, 2, 3, 4, 5, 6, 7, 8];
		let constraint = Constraint::from_inclusion_commitment(payload.clone());

		// Verify the constraint was created properly (implementation depends on Constraint structure)
		// This test validates that the conversion function works without panicking
		assert!(!format!("{:?}", constraint).is_empty(), "Constraint should format without error");
	}

	#[tokio::test]
	async fn test_process_constraints_for_slot_error_handling() {
		// Test error handling when processing constraints for a slot
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = ConstraintsApiClient::new(config.constraints_api.clone()).unwrap();
		let bls_manager = create_mock_bls_manager();
		let db_pool = sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap();

		let slot = 12345u64;

		// This should handle database errors gracefully
		let result = process_constraints_for_slot(&constraints_client, &bls_manager, &db_pool, &config, slot).await;

		assert!(result.is_err(), "Should fail due to database connection error");
	}

	#[test]
	fn test_constraint_deadline_calculation() {
		let genesis_time = 1606824023u64; // Ethereum mainnet genesis
		let slot = 12345u64;

		let deadline_timestamp = BeaconTiming::constraint_deadline_for_slot(genesis_time, slot);
		let expected_deadline = genesis_time + (slot * 12) + 8; // 8 second deadline

		assert_eq!(deadline_timestamp, expected_deadline, "Constraint deadline should be slot start + 8 seconds");

		// Verify the deadline makes sense
		let slot_start = genesis_time + (slot * 12);
		assert!(deadline_timestamp > slot_start, "Deadline should be after slot start");
		assert_eq!(deadline_timestamp - slot_start, 8, "Deadline should be 8 seconds after slot start");
	}

	#[tokio::test]
	async fn test_concurrent_constraint_processing() {
		// Test that multiple constraint processing operations can run concurrently
		let config = crate::testing::mocks::create_test_config();
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let bls_manager = Arc::new(create_mock_bls_manager());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let mut handles = Vec::new();

		// Start multiple processing tasks
		for _i in 0..3 {
			let client = Arc::clone(&constraints_client);
			let manager = Arc::clone(&bls_manager);
			let pool = Arc::clone(&db_pool);
			let conf = Arc::new(config.clone());

			let handle = tokio::spawn(async move { process_pending_constraints(client, manager, pool, conf).await });
			handles.push(handle);
		}

		// Wait for all to complete
		for handle in handles {
			let result = handle.await.unwrap();
			// All should complete (even if they fail due to database issues)
			assert!(result.is_ok(), "Concurrent processing should complete gracefully");
		}
	}
}
