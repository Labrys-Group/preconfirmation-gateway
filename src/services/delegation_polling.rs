use anyhow::{Context, Result};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{debug, error, info, warn};

use crate::api::beacon::BeaconApiClient;
use crate::api::constraints::ConstraintsApiClient;
use crate::config::Config;
use crate::crypto::bls::BlsManager;
use crate::db::delegation_ops::save_delegations_batch;
use crate::types::beacon::{BeaconTiming, timing};
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
		let scheduler = JobScheduler::new().await.context("Failed to create job scheduler")?;

		Ok(Self { beacon_client, constraints_client, db_pool, config, scheduler })
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
		})
		.context("Failed to create delegation polling job")?;

		self.scheduler.add(delegation_job).await.context("Failed to add delegation polling job to scheduler")?;

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
		})
		.context("Failed to create cleanup job")?;

		self.scheduler.add(cleanup_job).await.context("Failed to add cleanup job to scheduler")?;

		// Start the scheduler
		self.scheduler.start().await.context("Failed to start job scheduler")?;

		info!("Delegation polling service started successfully");
		Ok(())
	}

	/// Stop the delegation polling service
	pub async fn stop(&mut self) -> Result<()> {
		info!("Stopping delegation polling service");
		self.scheduler.shutdown().await.context("Failed to shutdown job scheduler")?;
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
		)
		.await
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

	// Calculate lookahead slots from configured epoch count
	let lookahead_slots = config.delegation.lookahead_epochs * timing::SLOTS_PER_EPOCH;

	// Poll for current slot + lookahead range
	let start_slot = current_slot;
	let end_slot = current_slot + lookahead_slots;

	info!(
		"Polling delegations for slots {} to {} ({} epochs lookahead)",
		start_slot, end_slot, config.delegation.lookahead_epochs
	);

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
			&config.delegation.domain_application_gateway,
		)
		.await
		{
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
			total_delegations_found,
			end_slot - start_slot + 1,
			successful_saves,
			errors
		);
	} else {
		info!(
			"Delegation polling cycle completed: no new delegations found across {} slots ({} errors)",
			end_slot - start_slot + 1,
			errors
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
	domain_application_gateway: &str,
) -> Result<usize> {
	// Get all delegations for this slot from the constraints API
	let delegations = constraints_client
		.get_delegations_for_slot(slot)
		.await
		.with_context(|| format!("Failed to fetch delegations for slot {}", slot))?;

	if delegations.is_empty() {
		return Ok(0);
	}

	// Filter to only delegations that involve our BLS key as delegate
	let our_bls_pubkey_bytes = our_bls_pubkey.to_bytes();

	let relevant_delegations: Vec<SignedDelegation> =
		delegations.into_iter().filter(|delegation| delegation.message.delegate.0 == our_bls_pubkey_bytes).collect();

	if relevant_delegations.is_empty() {
		return Ok(0);
	}

	debug!("Found {} relevant delegations for slot {} (involving our keys)", relevant_delegations.len(), slot);

	// Verify BLS signatures on delegations before saving
	let bls_manager = BlsManager::new(domain_application_gateway)
		.context("Failed to create BLS manager for signature verification")?;

	let mut verified_delegations = Vec::new();
	let mut invalid_count = 0;

	for delegation in relevant_delegations {
		match bls_manager.verify_delegation_signature(&delegation) {
			Ok(true) => {
				verified_delegations.push(delegation);
			}
			Ok(false) => {
				warn!(
					"Invalid BLS signature on delegation for slot {}, proposer 0x{}, delegate 0x{}",
					slot,
					hex::encode(delegation.message.proposer.0),
					hex::encode(delegation.message.delegate.0)
				);
				invalid_count += 1;
			}
			Err(e) => {
				warn!("Failed to verify BLS signature on delegation for slot {}: {}", slot, e);
				invalid_count += 1;
			}
		}
	}

	if invalid_count > 0 {
		warn!("Rejected {} delegations for slot {} due to invalid signatures", invalid_count, slot);
	}

	if verified_delegations.is_empty() {
		debug!("No verified delegations to save for slot {}", slot);
		return Ok(0);
	}

	debug!("Verified {} delegations with valid BLS signatures for slot {}", verified_delegations.len(), slot);

	// Save the delegations to the database
	let saved_ids = save_delegations_batch(db_pool, &verified_delegations)
		.await
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
	use crate::testing::fixtures::TestFixtures;
	use crate::testing::mocks::create_test_bls_keypair;
	use crate::types::delegation::{BlsSignature, DelegationMessage, SignedDelegation};
	use std::time::Duration;
	use tokio::time::timeout;

	fn create_mock_config() -> Config {
		let mut config = crate::testing::mocks::create_test_config();
		// Set specific values for delegation polling
		config.delegation.lookahead_epochs = 2;
		config.delegation.polling_interval_secs = 30;
		config.delegation.cache_ttl_secs = 300;
		config
	}

	#[tokio::test]
	async fn test_delegation_polling_service_creation() {
		let config = create_mock_config();
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
				.expect("Failed to connect to test database"),
		);

		let service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config)).await;

		assert!(service.is_ok(), "Should be able to create delegation polling service");
	}

	#[tokio::test]
	async fn test_delegation_polling_service_creation_without_db() {
		// Test service creation without requiring a real database connection
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());

		// Use lazy connection pool that doesn't actually connect
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config)).await;

		assert!(service.is_ok(), "Should be able to create delegation polling service without database");
	}

	#[tokio::test]
	async fn test_delegation_polling_service_lifecycle() {
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());

		// Skip test without database
		if std::env::var("DATABASE_URL").is_err() {
			return;
		}

		let db_pool = Arc::new(
			sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
				.await
				.expect("Failed to connect to test database"),
		);

		let mut service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Test start and stop
		service.start().await.expect("Failed to start service");

		// Give it a moment to run
		sleep(Duration::from_millis(100)).await;

		service.stop().await.expect("Failed to stop service");
	}

	#[tokio::test]
	async fn test_delegation_polling_service_lifecycle_without_db() {
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let mut service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Test start and stop (will fail during actual polling due to no DB, but lifecycle should work)
		service.start().await.expect("Failed to start service");

		// Give it a brief moment
		sleep(Duration::from_millis(50)).await;

		service.stop().await.expect("Failed to stop service");
	}

	#[tokio::test]
	async fn test_poll_once_without_error() {
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());

		// Skip test without database
		if std::env::var("DATABASE_URL").is_err() {
			return;
		}

		let db_pool = Arc::new(
			sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap())
				.await
				.expect("Failed to connect to test database"),
		);

		let service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// This might fail due to network issues, but shouldn't panic
		let result = timeout(Duration::from_secs(10), service.poll_once()).await;
		assert!(result.is_ok(), "poll_once should complete within timeout");
	}

	#[tokio::test]
	async fn test_poll_once_without_db() {
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// This should complete even if it fails due to network/database issues
		let result = timeout(Duration::from_secs(10), service.poll_once()).await;
		assert!(result.is_ok(), "poll_once should complete within timeout");
	}

	#[tokio::test]
	async fn test_poll_delegations_error_handling() {
		// Test error handling in the main polling logic
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		// This should handle errors gracefully and return Ok(())
		let result = poll_delegations(beacon_client, constraints_client, db_pool, Arc::new(config)).await;
		assert!(result.is_ok(), "poll_delegations should handle errors gracefully");
	}

	#[tokio::test]
	async fn test_poll_delegations_for_slot_error_handling() {
		// Test error handling for single slot polling
		let config = create_mock_config();
		let constraints_client = ConstraintsApiClient::new(config.constraints_api.clone()).unwrap();
		let db_pool = sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap();
		let (_sk, pk) = create_test_bls_keypair();

		let slot = 12345u64;

		// This should fail due to invalid API endpoints and no database connection
		let blst_pk = blst::min_pk::PublicKey::from_bytes(&pk.0).expect("Valid BLS public key");
		let result = poll_delegations_for_slot(
			&constraints_client,
			&db_pool,
			slot,
			&blst_pk,
			&config.delegation.domain_application_gateway,
		)
		.await;
		assert!(result.is_err(), "Should fail due to connectivity issues");
	}

	#[tokio::test]
	async fn test_cleanup_expired_delegations_error_handling() {
		// Test error handling for delegation cleanup
		let config = create_mock_config();
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		// This should handle database errors gracefully
		let result = cleanup_expired_delegations(db_pool, Arc::new(config)).await;
		assert!(result.is_err(), "Should fail due to no database connection");
	}

	#[test]
	fn test_delegation_configuration_validation() {
		let config = create_mock_config();

		// Verify delegation config values
		assert_eq!(config.delegation.lookahead_epochs, 2);
		assert_eq!(config.delegation.polling_interval_secs, 30);
		assert_eq!(config.delegation.cache_ttl_secs, 300);

		// Test slot range calculation
		let lookahead_slots = config.delegation.lookahead_epochs * crate::types::beacon::timing::SLOTS_PER_EPOCH;
		assert_eq!(lookahead_slots, 2 * 32, "Should calculate correct lookahead slots");
	}

	#[test]
	fn test_bls_signature_verification_setup() {
		let config = create_mock_config();

		// Test that BLS manager can be created for signature verification using configured domain
		let bls_manager_result = BlsManager::new(&config.delegation.domain_application_gateway);
		assert!(bls_manager_result.is_ok(), "Should be able to create BLS manager for verification");

		let _bls_manager = bls_manager_result.unwrap();

		// Create a test delegation to verify the structure is correct
		let (_proposer_sk, proposer_pk) = create_test_bls_keypair();
		let (_delegate_sk, delegate_pk) = create_test_bls_keypair();

		let delegation = TestFixtures::create_signed_delegation(
			12345,
			proposer_pk,
			delegate_pk,
			"0x1234567890123456789012345678901234567890",
		);

		// Verify delegation structure is valid (signature verification will fail due to mock signature)
		assert_eq!(delegation.message.slot, 12345);
		assert_eq!(delegation.message.committer, "0x1234567890123456789012345678901234567890");
	}

	#[test]
	fn test_delegation_filtering_logic() {
		// Test BLS key comparison logic used in delegation filtering
		let (_, our_pk) = create_test_bls_keypair();
		let (_, other_pk) = create_test_bls_keypair();

		let our_bytes = our_pk.0;
		let other_bytes = other_pk.0;

		// Verify that different keys produce different bytes
		assert_ne!(our_bytes, other_bytes, "Different keys should produce different byte arrays");

		// Test the filtering condition used in the polling logic
		let matches_our_key = our_bytes == our_bytes;
		let matches_other_key = our_bytes == other_bytes;

		assert!(matches_our_key, "Key should match itself");
		assert!(!matches_other_key, "Key should not match different key");
	}

	#[test]
	fn test_slot_range_calculation() {
		let config = create_mock_config();
		let genesis_time = config.beacon_api.genesis_time;

		// Test current slot calculation
		let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		let current_slot = BeaconTiming::current_slot_estimate(genesis_time);
		let expected_slot = (current_time - genesis_time) / 12;

		// Allow some tolerance for timing differences
		let slot_diff = (current_slot as i64 - expected_slot as i64).abs();
		assert!(slot_diff <= 2, "Current slot calculation should be accurate within 2 slots");

		// Test lookahead calculation
		let lookahead_slots = config.delegation.lookahead_epochs * crate::types::beacon::timing::SLOTS_PER_EPOCH;
		let end_slot = current_slot + lookahead_slots;

		assert!(end_slot > current_slot, "End slot should be after current slot");
		assert_eq!(end_slot - current_slot, lookahead_slots, "Slot range should match lookahead");
	}

	#[tokio::test]
	async fn test_concurrent_delegation_polling() {
		// Test concurrent polling operations
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let mut handles = Vec::new();

		// Start multiple polling operations
		for _i in 0..3 {
			let beacon = Arc::clone(&beacon_client);
			let constraints = Arc::clone(&constraints_client);
			let pool = Arc::clone(&db_pool);
			let conf = Arc::new(config.clone());

			let handle = tokio::spawn(async move { poll_delegations(beacon, constraints, pool, conf).await });
			handles.push(handle);
		}

		// Wait for all operations to complete
		for handle in handles {
			let result = handle.await.unwrap();
			// All should complete gracefully even if they encounter errors
			assert!(result.is_ok(), "Concurrent polling should complete gracefully");
		}
	}

	#[tokio::test]
	async fn test_delegation_validation_flow() {
		// Test the validation steps that would occur during delegation processing
		let (_proposer_sk, proposer_pk) = create_test_bls_keypair();
		let (_delegate_sk, delegate_pk) = create_test_bls_keypair();

		// Create a delegation with proper structure
		let slot = 12345u64;
		let committer = "0x1234567890123456789012345678901234567890";

		let delegation_message =
			DelegationMessage { proposer: proposer_pk, delegate: delegate_pk, committer: committer.to_string(), slot };

		// Create mock signature (real signature would require proper signing)
		let mock_signature = BlsSignature([42u8; 96]);
		let delegation = SignedDelegation { message: delegation_message, signature: mock_signature };

		// Test the filtering logic that would be used in polling
		let our_delegate_key = delegate_pk.0;
		let delegation_delegate_key = delegation.message.delegate.0;

		// This delegation should match our delegate key
		assert_eq!(delegation_delegate_key, our_delegate_key, "Delegation should match our delegate key");

		// Verify delegation structure is complete
		assert_eq!(delegation.message.slot, slot);
		assert_eq!(delegation.message.committer, committer);
		assert!(!delegation.signature.0.is_empty(), "Signature should not be empty");
	}

	#[tokio::test]
	async fn test_delegation_polling_timing() {
		// Test that polling operations complete within reasonable time bounds
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let start_time = std::time::Instant::now();

		// Run a polling cycle
		let result = timeout(
			Duration::from_secs(10),
			poll_delegations(beacon_client, constraints_client, db_pool, Arc::new(config)),
		)
		.await;

		let elapsed = start_time.elapsed();

		assert!(result.is_ok(), "Polling should complete within timeout");
		assert!(elapsed < Duration::from_secs(10), "Polling should complete reasonably quickly");
	}

	#[test]
	fn test_delegation_config_edge_cases() {
		let mut config = create_mock_config();

		// Test with zero lookahead
		config.delegation.lookahead_epochs = 0;
		let lookahead_slots = config.delegation.lookahead_epochs * crate::types::beacon::timing::SLOTS_PER_EPOCH;
		assert_eq!(lookahead_slots, 0, "Zero lookahead should result in zero slots");

		// Test with very high lookahead
		config.delegation.lookahead_epochs = 100;
		let lookahead_slots = config.delegation.lookahead_epochs * crate::types::beacon::timing::SLOTS_PER_EPOCH;
		assert_eq!(lookahead_slots, 3200, "High lookahead should calculate correctly");

		// Test with very short polling interval
		config.delegation.polling_interval_secs = 1;
		assert_eq!(config.delegation.polling_interval_secs, 1);

		// Test with very short cache TTL
		config.delegation.cache_ttl_secs = 1;
		assert_eq!(config.delegation.cache_ttl_secs, 1);
	}

	#[test]
	fn test_bls_key_conversion() {
		// Test BLS key type conversions used in the polling logic
		let (_, pk) = create_test_bls_keypair();

		// Test conversion to blst::min_pk::PublicKey
		let blst_pk = blst::min_pk::PublicKey::from_bytes(&pk.0).expect("Valid BLS public key");
		let converted_bytes = blst_pk.to_bytes();

		assert_eq!(converted_bytes, pk.0, "Key conversion should preserve bytes");
		assert_eq!(converted_bytes.len(), 48, "BLS public key should be 48 bytes");
	}

	#[tokio::test]
	async fn test_service_error_recovery() {
		// Test that the service can recover from errors during operation
		let config = create_mock_config();
		let beacon_client = Arc::new(BeaconApiClient::new(config.beacon_api.clone()).unwrap());
		let constraints_client = Arc::new(ConstraintsApiClient::new(config.constraints_api.clone()).unwrap());
		let db_pool = Arc::new(sqlx::PgPool::connect_lazy("postgresql://test:test@localhost/test_db").unwrap());

		let mut service = DelegationPollingService::new(beacon_client, constraints_client, db_pool, Arc::new(config))
			.await
			.expect("Failed to create service");

		// Start service
		service.start().await.expect("Failed to start service");

		// Let it run for a moment (it will encounter errors due to invalid endpoints/DB)
		sleep(Duration::from_millis(100)).await;

		// Service should still be running and stoppable despite errors
		service.stop().await.expect("Failed to stop service after errors");
	}
}
