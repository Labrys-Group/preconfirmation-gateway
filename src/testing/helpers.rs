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
	/// Creates a TestEnvironment populated with mock services for testing.
	///
	/// The returned TestEnvironment contains an Arc-wrapped test Config and mock
	/// implementations for the database, constraints API client, and beacon API
	/// client, suitable for use in unit and integration tests.
	///
	/// # Examples
	///
	/// ```ignore
	/// #[tokio::test]
	/// async fn setup_env_smoke() {
	///     let env = crate::testing::helpers::setup_test_environment().await;
	///     // Use `env` in test setup; fields are Arc-wrapped mock instances.
	///     assert!(std::sync::Arc::strong_count(&env.config) >= 1);
	/// }
	/// ```ignore
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

	/// Constructs an Arc-wrapped RpcContext preconfigured for testing.
	///
	/// The returned context contains a test database connection, a fee pricing engine, and a beacon API client wired from the provided configuration; it is intended for use in unit and integration tests that need a ready-to-use RpcContext.
	///
	/// # Arguments
	///
	/// * `config` - Shared configuration used to initialize clients and services within the test context.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::sync::Arc;
	///
	/// // construct or obtain a test Config
	/// let cfg = Arc::new(crate::config::Config::default());
	/// let rpc_ctx = crate::testing::helpers::create_test_rpc_context(cfg);
	/// // use `rpc_ctx` in tests...
	/// ```ignore
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

	/// Measures the elapsed time of an asynchronous operation and returns its result with the duration.
	///
	/// The returned tuple contains the operation's output as the first element and the elapsed `Duration` as the second.
	///
	/// # Examples
	///
	/// ```ignore
	/// # async fn example() {
	/// let (value, elapsed) = measure_time(|| async { 42 }).await;
	/// assert_eq!(value, 42);
	/// assert!(elapsed.as_nanos() >= 0);
	/// # }
	/// ```ignore
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

	/// Run an async operation and fail if it does not complete within the given duration.
	///
	/// The function awaits the provided operation and returns its result if it completes before
	/// `duration` elapses; otherwise it returns an error indicating a timeout.
	///
	/// # Returns
	///
	/// `Ok` containing the operation's result if it completed in time, or `Err` if the timeout was reached.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	/// use tokio::time::sleep;
	///
	/// #[tokio::main]
	/// async fn main() {
	///     // completes before timeout
	///     let res = crate::with_timeout(Duration::from_millis(200), || async {
	///         sleep(Duration::from_millis(50)).await;
	///         42
	///     }).await;
	///     assert_eq!(res.unwrap(), 42);
	/// }
	/// ```ignore
	pub async fn with_timeout<F, Fut, T>(duration: Duration, operation: F) -> Result<T>
	where
		F: FnOnce() -> Fut,
		Fut: std::future::Future<Output = T>,
	{
		timeout(duration, operation()).await.map_err(|_| anyhow::anyhow!("Operation timed out after {:?}", duration))
	}

	/// Ensures an asynchronous operation finishes within the specified duration.
	///
	/// Returns the operation's successful value if it completes within `max_duration`; returns an error if the operation exceeds the time limit.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	/// use anyhow::Result;
	/// use crate::testing::helpers::TestHelpers;
	///
	/// # async fn example() -> Result<()> {
	/// let value = TestHelpers::assert_completes_within(Duration::from_secs(1), || async {
	///     // perform async work...
	///     Ok::<_, anyhow::Error>(123)
	/// }).await?;
	///
	/// assert_eq!(value, 123);
	/// # Ok(())
	/// # }
	/// ```ignore
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

	/// Runs multiple asynchronous operations in parallel and returns their outputs with the total elapsed time.
	///
	/// The provided operations are executed concurrently (each spawned onto the Tokio runtime). The function
	/// waits for all operations to complete, panics if any spawned task panics, and returns a tuple with:
	/// 1) a `Vec<T>` containing each operation's output in the same order as the input operations, and
	/// 2) the total `Duration` elapsed from before spawning until all tasks have completed.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	///
	/// # tokio_test::block_on(async {
	/// let ops: Vec<Box<dyn FnOnce() -> _ + Send>> = vec![
	///     Box::new(|| async { 1 }),
	///     Box::new(|| async { 2 }),
	/// ];
	/// // Convert boxed closures to concrete typed vec for the function call
	/// let ops_typed: Vec<_> = ops.into_iter().map(|b| {
	///     // Re-box into the expected concrete type: FnOnce() -> impl Future<Output = i32> + Send + 'static
	///     move || (b)()
	/// }).collect();
	///
	/// let (results, duration): (Vec<i32>, Duration) = crate::testing::helpers::run_concurrent_operations(ops_typed).await;
	/// assert_eq!(results, vec![1, 2]);
	/// assert!(duration.as_millis() >= 0);
	/// # })
	/// ```ignore
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

	/// Runs concurrent simple queries against the provided Postgres pool and returns per-task metrics.
	///
	/// Spawns `concurrent_operations` tasks; each task executes `operations_per_task` `SELECT 1` queries
	/// with a brief pause between iterations. Returns a `DatabaseStressTestResults` containing the total
	/// duration, per-task success/failure counts, and the test configuration values.
	///
	/// # Examples
	///
	/// ```ignore
	/// #[tokio::test]
	/// async fn example_stress_test_database() {
	///     let pool = sqlx::postgres::PgPoolOptions::new()
	///         .max_connections(5)
	///         .connect_lazy("postgres://user:pass@localhost/db");
	///
	///     let results = crate::testing::helpers::stress_test_database(&pool, 2, 5).await.unwrap();
	///     assert_eq!(results.concurrent_operations, 2);
	///     assert_eq!(results.operations_per_task, 5);
	/// }
	/// ```ignore
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

	/// Ensures a constraint submission occurred no later than the slot's deadline derived from genesis.
	///
	/// The submission deadline is computed as: `genesis_time + (commitment_slot * 12) + 8` seconds.
	/// `commitment_slot` is the slot index, `submission_time` is the Instant when the submission was recorded,
	/// and `genesis_time` is the UNIX timestamp in seconds for genesis.
	///
	/// Returns `Ok(())` when the submission time is on or before the deadline. Returns an `Err` when the
	/// submission occurs after the deadline or when converting `Instant` to a UNIX timestamp overflows/underflows.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Instant;
	/// // genesis_time in seconds (UNIX timestamp)
	/// let genesis_time = 0_u64;
	/// let commitment_slot = 1_u64;
	/// let submission_time = Instant::now();
	/// assert!(crate::testing::helpers::validate_constraint_timing(commitment_slot, submission_time, genesis_time).is_ok());
	/// ```ignore
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

	/// Generates realistic inclusion-commitment load against the provided RPC context for the given duration and target throughput.
	///
	/// Repeatedly builds realistic commitment requests and simulates their submission, collecting per-request response times and counts of successful and failed requests. The returned `LoadTestResults` contains the observed duration, request counts, recorded response times, the configured target TPS, and the measured actual TPS.
	///
	/// # Examples
	///
	/// ```ignore
	/// #[tokio::test]
	/// async fn example_generate_load() {
	///     use std::sync::Arc;
	///     use std::time::Duration;
	///
	///     // Create a test RPC context (helper shown for illustration; adjust to your test setup).
	///     let config = Arc::new(crate::Config::default());
	///     let ctx = crate::testing::helpers::TestHelpers::create_test_rpc_context(config);
	///
	///     let results = crate::testing::helpers::TestHelpers::generate_realistic_load(ctx, Duration::from_secs(1), 5.0)
	///         .await
	///         .unwrap();
	///
	///     // At least one request should have been attempted during the interval.
	///     assert!(results.successful_requests + results.failed_requests > 0);
	/// }
	/// ```ignore
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

	/// Simulates handling a commitment request and returns a mocked signed commitment for use in tests.
	///
	/// Validates the request payload slot and generates a request hash, then constructs a `Commitment` and returns a `SignedCommitment` containing that commitment and a deterministic mock signature.
	///
	/// # Examples
	///
	/// ```ignore
	/// # async fn example() -> anyhow::Result<()> {
	/// use std::sync::Arc;
	/// // Construct a minimal `CommitmentRequest` appropriate to your test harness.
	/// // The exact fields depend on your crate's types; this shows the call pattern.
	/// let ctx = Arc::new(/* RpcContext for tests */);
	/// let request = /* build a CommitmentRequest for testing */;
	///
	/// let signed = crate::testing::helpers::simulate_commitment_request(ctx, request).await?;
	/// assert!(signed.signature.starts_with("0x"));
	/// # Ok(())
	/// # }
	/// ```ignore
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
	/// Returns the total number of operations executed across all tasks.
	///
	/// Sums each task's `successful_operations` and `failed_operations` in `task_results`.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	///
	/// let r1 = TaskResults { task_id: 0, successful_operations: 5, failed_operations: 2, total_duration: Duration::from_secs(1) };
	/// let r2 = TaskResults { task_id: 1, successful_operations: 3, failed_operations: 4, total_duration: Duration::from_secs(2) };
	/// let stats = DatabaseStressTestResults {
	///     total_duration: Duration::from_secs(3),
	///     task_results: vec![r1, r2],
	///     concurrent_operations: 2,
	///     operations_per_task: 7,
	/// };
	///
	/// assert_eq!(stats.total_operations(), 14);
	/// ```ignore
	pub fn total_operations(&self) -> usize {
		self.task_results.iter().map(|r| r.successful_operations + r.failed_operations).sum()
	}

	/// Returns the fraction of successful operations across all tasks.
	///
	/// The value is the number of successful operations divided by the total number
	/// of operations (successful + failed). If there are no operations in total,
	/// the result is `NaN`.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	///
	/// let results = crate::testing::helpers::DatabaseStressTestResults {
	///     total_duration: Duration::from_secs(1),
	///     task_results: vec![crate::testing::helpers::TaskResults {
	///         task_id: 0,
	///         successful_operations: 5,
	///         failed_operations: 5,
	///         total_duration: Duration::from_secs(1),
	///     }],
	///     concurrent_operations: 1,
	///     operations_per_task: 10,
	/// };
	///
	/// assert_eq!(results.success_rate(), 0.5);
	///
	/// let empty = crate::testing::helpers::DatabaseStressTestResults {
	///     total_duration: Duration::from_secs(0),
	///     task_results: vec![],
	///     concurrent_operations: 0,
	///     operations_per_task: 0,
	/// };
	///
	/// assert!(empty.success_rate().is_nan());
	/// ```ignore
	pub fn success_rate(&self) -> f64 {
		let total_success: usize = self.task_results.iter().map(|r| r.successful_operations).sum();
		total_success as f64 / self.total_operations() as f64
	}

	/// Calculates the achieved operations per second.
	///
	/// Returns the total number of operations (successful + failed) divided by the total duration in seconds. If `total_duration` is zero, this will return `f64::INFINITY`.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	/// let task = TaskResults {
	///     task_id: 0,
	///     successful_operations: 100,
	///     failed_operations: 0,
	///     total_duration: Duration::from_secs(10),
	/// };
	/// let results = DatabaseStressTestResults {
	///     total_duration: Duration::from_secs(10),
	///     task_results: vec![task],
	///     concurrent_operations: 1,
	///     operations_per_task: 100,
	/// };
	/// assert_eq!(results.operations_per_second(), 10.0);
	/// ```ignore
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
	/// Calculates the fraction of requests that succeeded.
	///
	/// If no requests were made (successful_requests + failed_requests == 0), this returns `NaN`.
	///
	/// # Examples
	///
	/// ```ignore
	/// let results = LoadTestResults {
	///     duration: std::time::Duration::from_secs(1),
	///     successful_requests: 80,
	///     failed_requests: 20,
	///     response_times: vec![],
	///     target_tps: 100.0,
	///     actual_tps: 100.0,
	/// };
	/// assert!((results.success_rate() - 0.8).abs() < 1e-12);
	///
	/// let empty = LoadTestResults {
	///     duration: std::time::Duration::from_secs(0),
	///     successful_requests: 0,
	///     failed_requests: 0,
	///     response_times: vec![],
	///     target_tps: 0.0,
	///     actual_tps: 0.0,
	/// };
	/// assert!(empty.success_rate().is_nan());
	/// ```ignore
	pub fn success_rate(&self) -> f64 {
		self.successful_requests as f64 / (self.successful_requests + self.failed_requests) as f64
	}

	/// Computes the average of the recorded response times.
	///
	/// If there are no recorded response times, returns a zero `Duration`.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	/// # // Minimal shim of the struct for the doctest
	/// # struct LoadTestResults { response_times: Vec<Duration> }
	/// # impl LoadTestResults {
	/// #     fn average_response_time(&self) -> Duration {
	/// #         if self.response_times.is_empty() {
	/// #             return Duration::default();
	/// #         }
	/// #         let total: Duration = self.response_times.iter().sum();
	/// #         total / self.response_times.len() as u32
	/// #     }
	/// # }
	/// let results = LoadTestResults {
	///     response_times: vec![Duration::from_millis(100), Duration::from_millis(200)],
	/// };
	/// assert_eq!(results.average_response_time(), Duration::from_millis(150));
	/// ```ignore
	pub fn average_response_time(&self) -> Duration {
		if self.response_times.is_empty() {
			return Duration::default();
		}

		let total: Duration = self.response_times.iter().sum();
		total / self.response_times.len() as u32
	}

	/// Returns the response time at the given percentile (percentage).
	///
	/// The method clones and sorts recorded response times and selects the value at
	/// position floor(n * percentile / 100). If no response times are available,
	/// returns a zero `Duration`.
	///
	/// `percentile` must be in the range 0.0..=100.0; behavior for values outside
	/// that range is undefined.
	///
	/// # Examples
	///
	/// ```ignore
	/// use std::time::Duration;
	///
	/// let results = LoadTestResults {
	///     duration: Duration::from_secs(1),
	///     successful_requests: 3,
	///     failed_requests: 0,
	///     response_times: vec![Duration::from_millis(10), Duration::from_millis(30), Duration::from_millis(20)],
	///     target_tps: 1.0,
	///     actual_tps: 3.0,
	/// };
	///
	/// // 50th percentile (median) should be 20 ms
	/// assert_eq!(results.percentile_response_time(50.0), Duration::from_millis(20));
	/// ```ignore
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
