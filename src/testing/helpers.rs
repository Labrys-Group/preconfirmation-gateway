use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

use crate::config::Config;
use crate::testing::mocks::{MockBeaconApiClient, MockConstraintsApiClient, MockDatabase};
use crate::types::{CommitmentRequest, RpcContext, SignedCommitment};

/// Test helper utilities
pub struct TestHelpers;

impl TestHelpers {
	/// Setup a test environment with mock services
	pub async fn setup_test_environment() -> TestEnvironment {
		let config = crate::testing::mocks::create_test_config();
		let mock_db = MockDatabase::new();
		let mock_constraints_client = MockConstraintsApiClient::new();
		let mock_beacon_client = MockBeaconApiClient::new();

		TestEnvironment {
			config: Arc::new(config),
			mock_db: Arc::new(mock_db),
			mock_constraints_client: Arc::new(mock_constraints_client),
			mock_beacon_client: Arc::new(mock_beacon_client),
		}
	}

	/// Create a test RPC context
	pub fn create_test_rpc_context(config: Arc<Config>) -> Arc<RpcContext> {
		// Create a test database pool (using an in-memory SQLite for testing would be ideal,
		// but for now we'll create a minimal PgPool that won't actually connect)
		let database_url = "postgresql://test:test@localhost/test_db";
		let pool = sqlx::PgPool::connect_lazy(database_url).expect("Failed to create test pool");

		let database = crate::db::DatabaseContext::new(pool);

		// Create a minimal fee engine for testing
		let reth_config = crate::api::reth::RethApiConfig {
			endpoint: "http://localhost:8545".to_string(),
			request_timeout_secs: 30,
			max_retries: 3,
		};
		let reth_client = Arc::new(crate::api::reth::RethApiClient::new(reth_config).unwrap());
		let database_arc = Arc::new(database.clone());
		let config_arc = config.clone();
		let fee_engine = Arc::new(crate::services::fee_pricing::FeePricingEngine::new(
			reth_client,
			database_arc,
			config_arc.clone(),
		));

		// Create beacon API client for testing
		let beacon_client = Arc::new(crate::api::beacon::BeaconApiClient::new(config.beacon_api.clone()).unwrap());

		Arc::new(RpcContext::new(database, (*config).clone(), fee_engine, beacon_client))
	}

	/// Measure the execution time of an async operation
	pub async fn measure_time<F, Fut, T>(operation: F) -> (T, Duration)
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = T>,
	{
		let start = Instant::now();
		let result = operation().await;
		let duration = start.elapsed();
		(result, duration)
	}

	/// Run a test with timeout
	pub async fn with_timeout<F, Fut, T>(duration: Duration, operation: F) -> Result<T>
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = T>,
	{
		timeout(duration, operation()).await.map_err(|_| anyhow::anyhow!("Operation timed out after {:?}", duration))
	}

	/// Assert that an operation completes within a time limit
	pub async fn assert_completes_within<F, Fut, T>(max_duration: Duration, operation: F) -> Result<T>
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = Result<T>>,
	{
		let (result, actual_duration) = Self::measure_time(operation).await;

		if actual_duration > max_duration {
			return Err(anyhow::anyhow!(
				"Operation took {:?}, which exceeds max duration of {:?}",
				actual_duration,
				max_duration
			));
		}

		result
	}

	/// Run multiple operations concurrently and measure total time
	pub async fn run_concurrent_operations<F, Fut, T>(operations: Vec<F>) -> (Vec<T>, Duration)
	where
		F: FnOnce() -> Fut + Send + 'static,
		Fut: std::future::Future<Output = T> + Send + 'static,
		T: Send + 'static,
	{
		let start = Instant::now();

		let handles: Vec<_> = operations.into_iter().map(|op| tokio::spawn(op())).collect();

		let results =
			futures::future::join_all(handles).await.into_iter().map(|result| result.expect("Task panicked")).collect();

		let duration = start.elapsed();
		(results, duration)
	}

	/// Test database operations under load
	pub async fn stress_test_database(
		pool: &PgPool,
		concurrent_operations: usize,
		operations_per_task: usize,
	) -> Result<DatabaseStressTestResults> {
		let start = Instant::now();
		let mut handles = Vec::new();

		for task_id in 0..concurrent_operations {
			let pool_clone = pool.clone();
			let handle = tokio::spawn(async move {
				let mut task_results = TaskResults {
					task_id,
					successful_operations: 0,
					failed_operations: 0,
					total_duration: Duration::default(),
				};

				let task_start = Instant::now();

				for _op_id in 0..operations_per_task {
					// Simulate a database operation (e.g., checking delegation)
					let query_result = sqlx::query("SELECT 1 as test").fetch_one(&pool_clone).await;

					match query_result {
						Ok(_) => task_results.successful_operations += 1,
						Err(_) => task_results.failed_operations += 1,
					}

					// Small delay to prevent overwhelming the database
					tokio::time::sleep(Duration::from_millis(1)).await;
				}

				task_results.total_duration = task_start.elapsed();
				task_results
			});

			handles.push(handle);
		}

		let task_results =
			futures::future::join_all(handles).await.into_iter().map(|result| result.expect("Task panicked")).collect();

		let total_duration = start.elapsed();

		Ok(DatabaseStressTestResults { total_duration, task_results, concurrent_operations, operations_per_task })
	}

	/// Validate that constraint submission timing is correct
	pub fn validate_constraint_timing(commitment_slot: u64, submission_time: Instant, genesis_time: u64) -> Result<()> {
		// Convert Instant to unix timestamp by calculating offset from now
		let now_instant = Instant::now();
		let now_system = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();

		let submission_unix = if submission_time <= now_instant {
			// submission_time is in the past
			let elapsed_since_submission = now_instant.duration_since(submission_time);
			now_system
				.checked_sub(elapsed_since_submission)
				.ok_or_else(|| anyhow::anyhow!("Submission time calculation underflow"))?
				.as_secs()
		} else {
			// submission_time is in the future
			let elapsed_until_submission = submission_time.duration_since(now_instant);
			now_system
				.checked_add(elapsed_until_submission)
				.ok_or_else(|| anyhow::anyhow!("Submission time calculation overflow"))?
				.as_secs()
		};

		let slot_start_time = genesis_time + (commitment_slot * 12);
		let submission_deadline = slot_start_time + 8; // 8-second deadline

		if submission_unix > submission_deadline {
			return Err(anyhow::anyhow!(
				"Constraint submission too late: submitted at {}, deadline was {}",
				submission_unix,
				submission_deadline
			));
		}

		Ok(())
	}

	/// Generate test load with realistic patterns
	pub async fn generate_realistic_load(
		context: Arc<RpcContext>,
		duration: Duration,
		target_tps: f64,
	) -> Result<LoadTestResults> {
		let start = Instant::now();
		let interval = Duration::from_secs_f64(1.0 / target_tps);
		let mut successful_requests = 0;
		let mut failed_requests = 0;
		let mut response_times = Vec::new();

		while start.elapsed() < duration {
			let request_start = Instant::now();

			// Create a realistic commitment request
			let slot = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 12; // Current slot

			let commitment_request = crate::testing::fixtures::TestFixtures::create_inclusion_commitment_request(
				slot,
				&context.config.validation.slasher_address,
			);

			// Simulate the request
			let result = Self::simulate_commitment_request(Arc::clone(&context), commitment_request).await;

			let response_time = request_start.elapsed();
			response_times.push(response_time);

			match result {
				Ok(_) => successful_requests += 1,
				Err(_) => failed_requests += 1,
			}

			// Wait for next request
			tokio::time::sleep(interval).await;
		}

		Ok(LoadTestResults {
			duration: start.elapsed(),
			successful_requests,
			failed_requests,
			response_times,
			target_tps,
			actual_tps: successful_requests as f64 / start.elapsed().as_secs_f64(),
		})
	}

	/// Simulate a commitment request for testing
	async fn simulate_commitment_request(
		_context: Arc<RpcContext>,
		request: CommitmentRequest,
	) -> Result<SignedCommitment> {
		// For load testing, we need a fast simulation that exercises the core logic

		// Validate the request format
		let _slot = crate::types::payload::PayloadParser::extract_slot(request.commitment_type, &request.payload)
			.map_err(|e| anyhow::anyhow!("Invalid payload: {}", e))?;

		// Generate request hash
		let request_hash = crate::crypto::generate_request_hash(&request)?;

		// Create a test commitment
		let commitment = crate::types::Commitment {
			commitment_type: request.commitment_type,
			payload: request.payload.clone(),
			request_hash,
			slasher: request.slasher.clone(),
		};

		// Simulate signing delay (realistic timing)
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;

		// Return mock signed commitment
		Ok(SignedCommitment {
			commitment,
			signature: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
		})
	}
}

/// Test environment with mock services
pub struct TestEnvironment {
	pub config: Arc<Config>,
	pub mock_db: Arc<MockDatabase>,
	pub mock_constraints_client: Arc<MockConstraintsApiClient>,
	pub mock_beacon_client: Arc<MockBeaconApiClient>,
}

/// Results from database stress testing
#[derive(Debug)]
pub struct DatabaseStressTestResults {
	pub total_duration: Duration,
	pub task_results: Vec<TaskResults>,
	pub concurrent_operations: usize,
	pub operations_per_task: usize,
}

impl DatabaseStressTestResults {
	pub fn total_operations(&self) -> usize {
		self.task_results.iter().map(|r| r.successful_operations + r.failed_operations).sum()
	}

	pub fn success_rate(&self) -> f64 {
		let total_success: usize = self.task_results.iter().map(|r| r.successful_operations).sum();
		total_success as f64 / self.total_operations() as f64
	}

	pub fn operations_per_second(&self) -> f64 {
		self.total_operations() as f64 / self.total_duration.as_secs_f64()
	}
}

/// Results from individual task in stress test
#[derive(Debug)]
pub struct TaskResults {
	pub task_id: usize,
	pub successful_operations: usize,
	pub failed_operations: usize,
	pub total_duration: Duration,
}

/// Results from load testing
#[derive(Debug)]
pub struct LoadTestResults {
	pub duration: Duration,
	pub successful_requests: usize,
	pub failed_requests: usize,
	pub response_times: Vec<Duration>,
	pub target_tps: f64,
	pub actual_tps: f64,
}

impl LoadTestResults {
	pub fn success_rate(&self) -> f64 {
		self.successful_requests as f64 / (self.successful_requests + self.failed_requests) as f64
	}

	pub fn average_response_time(&self) -> Duration {
		if self.response_times.is_empty() {
			return Duration::default();
		}

		let total: Duration = self.response_times.iter().sum();
		total / self.response_times.len() as u32
	}

	pub fn percentile_response_time(&self, percentile: f64) -> Duration {
		if self.response_times.is_empty() {
			return Duration::default();
		}

		let mut sorted_times = self.response_times.clone();
		sorted_times.sort();

		let index = (sorted_times.len() as f64 * percentile / 100.0) as usize;
		sorted_times.get(index.min(sorted_times.len() - 1)).copied().unwrap_or_default()
	}
}
