use anyhow::Result;
use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use crate::api::beacon::BeaconApiClient;
use crate::api::constraints::{ConstraintSubmissionResponse, ConstraintsApiClient};
use crate::config::Config;
use crate::types::beacon::ProposerDutiesResponse;
use crate::types::delegation::{SignedConstraints, SignedDelegation};

/// Mock beacon API client for testing
pub struct MockBeaconApiClient {
	/// Predefined responses for proposer duties by epoch
	pub proposer_duties: Arc<RwLock<HashMap<u64, ProposerDutiesResponse>>>,
	/// Simulated network delays (in milliseconds)
	pub network_delay_ms: u64,
	/// Whether to simulate failures
	pub should_fail: bool,
}

impl Default for MockBeaconApiClient {
	/// Create a default instance of this type.
	///
	/// # Examples
	///
	/// ```ignore
	/// let _instance = Default::default();
	/// ```ignore
	fn default() -> Self {
		Self::new()
	}
}

impl MockBeaconApiClient {
	/// Creates a new in-memory MockBeaconApiClient with default test settings.
	///
	/// The default instance has an empty proposer duties map, a 50 ms simulated network delay,
	/// and failure simulation disabled.
	///
	/// # Examples
	///
	/// ```ignore
	/// let client = MockBeaconApiClient::new();
	/// assert_eq!(client.network_delay_ms, 50);
	/// assert_eq!(client.should_fail, false);
	/// ```ignore
	pub fn new() -> Self {
		Self { proposer_duties: Arc::new(RwLock::new(HashMap::new())), network_delay_ms: 50, should_fail: false }
	}

	/// Insert mock proposer duties for the specified epoch into the in-memory store.
	///
	/// This will replace any duties already stored for the same epoch.
	///
	/// # Parameters
	///
	/// - `epoch`: Epoch for which the proposer duties apply.
	/// - `duties`: The proposer duties to store.
	///
	/// # Examples
	///
	/// ```ignore
	/// # use testing::mocks::MockBeaconApiClient;
	/// # use crate::types::beacon::ProposerDutiesResponse;
	/// #[tokio::test]
	/// async fn add_duties_example() {
	///     let client = MockBeaconApiClient::new();
	///     let duties = ProposerDutiesResponse::default();
	///     client.add_proposer_duties(42, duties).await;
	///     // duties for epoch 42 are now stored in the mock
	/// }
	/// ```ignore
	pub async fn add_proposer_duties(&self, epoch: u64, duties: ProposerDutiesResponse) {
		self.proposer_duties.write().await.insert(epoch, duties);
	}

	/// Sets the simulated network delay (in milliseconds) used by the mock client.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut mock = MockBeaconApiClient::new();
	/// mock.set_network_delay(200);
	/// ```ignore
	pub fn set_network_delay(&mut self, delay_ms: u64) {
		self.network_delay_ms = delay_ms;
	}

	/// Enable or disable simulated failure mode for the mock.
	///
	/// When enabled (`should_fail = true`), mock operations that simulate external
	/// interactions will return errors instead of successful responses; when
	/// disabled they behave normally.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut mock = MockDatabase::new();
	/// mock.set_failure_mode(true);
	/// // subsequent mock calls will simulate failures
	/// ```ignore
	pub fn set_failure_mode(&mut self, should_fail: bool) {
		self.should_fail = should_fail;
	}
}

impl BeaconApiClient {
	/// Constructs a MockBeaconApiClient preconfigured for testing.
	///
	/// The returned mock simulates the Beacon API and exposes controls for network delay,
	/// failure mode, and injectable proposer duties.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut client = MockBeaconApiClient::mock();
	/// client.set_network_delay(10);
	/// client.set_failure_mode(false);
	/// ```ignore
	pub fn mock() -> MockBeaconApiClient {
		MockBeaconApiClient::new()
	}
}

/// Mock constraints API client for testing
pub struct MockConstraintsApiClient {
	/// Stored delegations by slot
	pub delegations: Arc<RwLock<HashMap<u64, Vec<SignedDelegation>>>>,
	/// Submitted constraints log
	pub submitted_constraints: Arc<Mutex<Vec<SignedConstraints>>>,
	/// Network delay simulation
	pub network_delay_ms: u64,
	/// Failure simulation
	pub should_fail: bool,
	/// Mock responses for constraint submission
	pub submission_responses: Arc<RwLock<Vec<ConstraintSubmissionResponse>>>,
}

impl Default for MockConstraintsApiClient {
	/// Create a default instance of this type.
	///
	/// # Examples
	///
	/// ```ignore
	/// let _instance = Default::default();
	/// ```ignore
	fn default() -> Self {
		Self::new()
	}
}

impl MockConstraintsApiClient {
	/// Create a new MockConstraintsApiClient initialized for tests.
	///
	/// The client starts with empty in-memory delegations and submission logs,
	/// a simulated network delay of 100 milliseconds, failure mode disabled, and
	/// no queued submission responses.
	///
	/// # Examples
	///
	/// ```ignore
	/// let client = MockConstraintsApiClient::new();
	/// // client can now be used to add delegations, submit constraints, or inspect submissions.
	/// ```ignore
	pub fn new() -> Self {
		Self {
			delegations: Arc::new(RwLock::new(HashMap::new())),
			submitted_constraints: Arc::new(Mutex::new(Vec::new())),
			network_delay_ms: 100,
			should_fail: false,
			submission_responses: Arc::new(RwLock::new(Vec::new())),
		}
	}

	/// Insert a signed delegation into the in-memory store for a given slot for testing.
	///
	/// Adds the provided `delegation` to the vector associated with `slot`, creating the vector if none exists.
	///
	/// # Examples
	///
	/// ```ignore
	/// use crate::testing::mocks::MockConstraintsApiClient;
	/// use crate::types::delegation::SignedDelegation;
	///
	/// #[tokio::test]
	/// async fn example_add_delegation() {
	///     let client = MockConstraintsApiClient::new();
	///     // Construct a test delegation; replace with a real SignedDelegation as needed.
	///     let delegation = SignedDelegation::default();
	///     client.add_delegation(42, delegation).await;
	///     let delegations = client.mock_get_delegations_for_slot(42).await.unwrap();
	///     assert_eq!(delegations.len(), 1);
	/// }
	/// ```ignore
	pub async fn add_delegation(&self, slot: u64, delegation: SignedDelegation) {
		self.delegations.write().await.entry(slot).or_insert_with(Vec::new).push(delegation);
	}

	/// Return a cloned list of all constraints that have been submitted to this mock.
	///
	/// The returned vector is a clone of the internal submission log and can be used
	/// for test assertions without affecting the mock's internal state.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mock = MockConstraintsApiClient::new();
	/// // initially empty
	/// assert_eq!(mock.get_submitted_constraints().len(), 0);
	/// ```ignore
	pub fn get_submitted_constraints(&self) -> Vec<SignedConstraints> {
		self.submitted_constraints.lock().unwrap().clone()
	}

	/// Clears the in-memory log of submitted constraints.
	///
	/// # Examples
	///
	/// ```ignore
	/// let client = MockConstraintsApiClient::new();
	/// // Ensure log is empty after clearing
	/// client.clear_submitted_constraints();
	/// assert_eq!(client.get_submitted_constraints().len(), 0);
	/// ```ignore
	pub fn clear_submitted_constraints(&self) {
		self.submitted_constraints.lock().unwrap().clear();
	}

	/// Sets the simulated network delay (in milliseconds) used by the mock client.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut mock = MockBeaconApiClient::new();
	/// mock.set_network_delay(200);
	/// ```ignore
	pub fn set_network_delay(&mut self, delay_ms: u64) {
		self.network_delay_ms = delay_ms;
	}

	/// Enable or disable simulated failure mode for the mock.
	///
	/// When enabled (`should_fail = true`), mock operations that simulate external
	/// interactions will return errors instead of successful responses; when
	/// disabled they behave normally.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut mock = MockDatabase::new();
	/// mock.set_failure_mode(true);
	/// // subsequent mock calls will simulate failures
	/// ```ignore
	pub fn set_failure_mode(&mut self, should_fail: bool) {
		self.should_fail = should_fail;
	}

	/// Appends a mock constraint submission response to the client's response queue.
	///
	/// The added response will be returned (in FIFO order) by subsequent submission calls on this mock client.
	///
	/// # Examples
	///
	/// ```ignoreno_run
	/// # async fn example() {
	/// let client = MockConstraintsApiClient::new();
	/// let response = ConstraintSubmissionResponse { /* fields */ };
	/// client.add_submission_response(response).await;
	/// # }
	/// ```ignore
	pub async fn add_submission_response(&self, response: ConstraintSubmissionResponse) {
		self.submission_responses.write().await.push(response);
	}

	/// Simulate submitting signed constraints to the mock Constraints API and record them for verification.
	///
	/// This method applies the configured network delay, optionally fails if failure mode is enabled,
	/// stores a clone of the provided `constraints` in the client's submitted-constraints log, and then
	/// returns a configured mock `ConstraintSubmissionResponse` or a default successful response if none
	/// are queued.
	///
	/// # Parameters
	///
	/// - `constraints`: The signed constraints to submit and record in the mock client's log.
	///
	/// # Returns
	///
	/// A `ConstraintSubmissionResponse` describing the simulated submission outcome (e.g., `success` and an optional `submission_id`).
	///
	/// # Examples
	///
	/// ```ignore
	/// # use crate::testing::mocks::MockConstraintsApiClient;
	/// # use crate::types::constraints::SignedConstraints;
	/// # tokio::test
	/// async fn example_mock_submit_constraints() {
	///     let client = MockConstraintsApiClient::new();
	///     let constraints = SignedConstraints::default();
	///     let resp = client.mock_submit_constraints(&constraints).await.unwrap();
	///     assert!(resp.success);
	///     assert_eq!(client.get_submitted_constraints().len(), 1);
	/// }
	/// ```ignore
	pub async fn mock_submit_constraints(
		&self,
		constraints: &SignedConstraints,
	) -> Result<ConstraintSubmissionResponse> {
		// Simulate network delay
		if self.network_delay_ms > 0 {
			tokio::time::sleep(tokio::time::Duration::from_millis(self.network_delay_ms)).await;
		}

		// Simulate failure if configured
		if self.should_fail {
			return Err(anyhow::anyhow!("Mock constraint submission failure"));
		}

		// Store the submitted constraints for verification
		self.submitted_constraints.lock().unwrap().push(constraints.clone());

		// Return a mock response
		let responses = self.submission_responses.read().await;
		if let Some(response) = responses.first() {
			Ok(response.clone())
		} else {
			// Default mock response
			Ok(ConstraintSubmissionResponse { success: true, submission_id: Some("test_submission_id".to_string()) })
		}
	}

	/// Fetches mock signed delegations stored for the specified slot.
	///
	/// Returns the stored delegations for `slot` or an empty vector if none exist.
	/// Returns an error when the mock is configured to simulate failure.
	///
	/// # Examples
	///
	/// ```ignore
	/// # tokio_test::block_on(async {
	/// let client = MockConstraintsApiClient::new();
	/// client.add_delegation(42, SignedDelegation::default()).await;
	/// let delegations = client.mock_get_delegations_for_slot(42).await.unwrap();
	/// assert!(!delegations.is_empty());
	/// # });
	/// ```ignore
	pub async fn mock_get_delegations_for_slot(&self, slot: u64) -> Result<Vec<SignedDelegation>> {
		// Simulate network delay
		if self.network_delay_ms > 0 {
			tokio::time::sleep(tokio::time::Duration::from_millis(self.network_delay_ms)).await;
		}

		// Simulate failure if configured
		if self.should_fail {
			return Err(anyhow::anyhow!("Mock delegation fetch failure for slot {}", slot));
		}

		let delegations = self.delegations.read().await;
		Ok(delegations.get(&slot).cloned().unwrap_or_default())
	}
}

impl ConstraintsApiClient {
	/// Create a mock Constraints API client for testing.
	///
	/// The returned client simulates network latency and failure modes and records submitted constraints.
	///
	/// # Examples
	///
	/// ```ignore
	/// let client = mock();
	/// assert!(client.get_submitted_constraints().is_empty());
	/// ```ignore
	pub fn mock() -> MockConstraintsApiClient {
		MockConstraintsApiClient::new()
	}
}

/// Mock database for testing
pub struct MockDatabase {
	/// In-memory storage for testing
	pub commitments: Arc<RwLock<HashMap<String, crate::types::SignedCommitment>>>,
	pub delegations: Arc<RwLock<HashMap<u64, Vec<SignedDelegation>>>>,
	/// Simulate database latency
	pub latency_ms: u64,
	/// Simulate database failures
	pub should_fail: bool,
}

impl Default for MockDatabase {
	/// Create a default instance of this type.
	///
	/// # Examples
	///
	/// ```ignore
	/// let _instance = Default::default();
	/// ```ignore
	fn default() -> Self {
		Self::new()
	}
}

impl MockDatabase {
	/// Create a new in-memory MockDatabase with empty storage, a 10ms simulated latency, and failure mode disabled.
	///
	/// # Examples
	///
	/// ```ignore
	/// let db = MockDatabase::new();
	/// // use `db` in tests to simulate persistence operations
	/// ```ignore
	pub fn new() -> Self {
		Self {
			commitments: Arc::new(RwLock::new(HashMap::new())),
			delegations: Arc::new(RwLock::new(HashMap::new())),
			latency_ms: 10,
			should_fail: false,
		}
	}

	/// Configure simulated database latency in milliseconds.
	///
	/// The configured value is applied to mock database operations to introduce an artificial delay
	/// for testing timing and retry behaviors.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut db = MockDatabase::new();
	/// db.set_latency(50);
	/// ```ignore
	pub fn set_latency(&mut self, latency_ms: u64) {
		self.latency_ms = latency_ms;
	}

	/// Enable or disable simulated failure mode for the mock.
	///
	/// When enabled (`should_fail = true`), mock operations that simulate external
	/// interactions will return errors instead of successful responses; when
	/// disabled they behave normally.
	///
	/// # Examples
	///
	/// ```ignore
	/// let mut mock = MockDatabase::new();
	/// mock.set_failure_mode(true);
	/// // subsequent mock calls will simulate failures
	/// ```ignore
	pub fn set_failure_mode(&mut self, should_fail: bool) {
		self.should_fail = should_fail;
	}

	/// Saves a signed commitment into the mock in-memory database, honoring the mock's configured latency and failure mode.
	///
	/// The method inserts the provided `SignedCommitment` into the database's in-memory commitments map keyed by the commitment's `request_hash`.
	///
	/// # Parameters
	///
	/// - `commitment`: The `SignedCommitment` to persist in the mock database.
	///
	/// # Returns
	///
	/// `Ok(())` on successful save, `Err(...)` when the mock is configured to simulate a failure.
	///
	/// # Examples
	///
	/// ```ignore
	/// #[tokio::test]
	/// async fn save_commitment_example() {
	///     use testing::mocks::MockDatabase;
	///     // Construct a default SignedCommitment for the example; in real tests provide a fully formed value.
	///     let db = MockDatabase::new();
	///     let commitment = crate::types::SignedCommitment::default();
	///     assert!(db.mock_save_commitment(&commitment).await.is_ok());
	/// }
	/// ```ignore
	pub async fn mock_save_commitment(&self, commitment: &crate::types::SignedCommitment) -> Result<()> {
		if self.latency_ms > 0 {
			tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
		}

		if self.should_fail {
			return Err(anyhow::anyhow!("Mock database save failure"));
		}

		self.commitments.write().await.insert(commitment.commitment.request_hash.clone(), commitment.clone());
		Ok(())
	}

	/// Retrieve a stored signed commitment by its request hash from the mock database.
	///
	/// The method will apply the mock database's configured latency before returning. If the mock's
	/// failure mode is enabled, this method returns an error.
	///
	/// # Returns
	///
	/// `Some(SignedCommitment)` if a commitment with the given hash exists, `None` otherwise.
	///
	/// # Errors
	///
	/// Returns an `Err` when the mock database is configured to fail.
	///
	/// # Examples
	///
	/// ```ignore
	/// #[tokio::test]
	/// async fn example_mock_get_commitment_returns_none() {
	///     let db = MockDatabase::new();
	///     let result = db.mock_get_commitment("missing-hash").await.unwrap();
	///     assert!(result.is_none());
	/// }
	/// ```ignore
	pub async fn mock_get_commitment(&self, hash: &str) -> Result<Option<crate::types::SignedCommitment>> {
		if self.latency_ms > 0 {
			tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
		}

		if self.should_fail {
			return Err(anyhow::anyhow!("Mock database get failure"));
		}

		Ok(self.commitments.read().await.get(hash).cloned())
	}

	/// Check whether a commitment with the given request hash exists in the mock database.
	///
	/// Returns an error if the mock database is configured to fail.
	///
	/// # Examples
	///
	/// ```ignore
	/// # use tokio::runtime::Runtime;
	/// # let rt = Runtime::new().unwrap();
	/// # rt.block_on(async {
	/// let db = crate::testing::mocks::MockDatabase::new();
	/// let commitment = crate::types::SignedCommitment::default();
	/// let hash = commitment.request_hash.clone();
	/// db.mock_save_commitment(&commitment).await.unwrap();
	/// assert!(db.mock_commitment_exists(&hash).await.unwrap());
	/// assert!(!db.mock_commitment_exists("nonexistent_hash").await.unwrap());
	/// # });
	/// ```ignore
	pub async fn mock_commitment_exists(&self, hash: &str) -> Result<bool> {
		if self.latency_ms > 0 {
			tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
		}

		if self.should_fail {
			return Err(anyhow::anyhow!("Mock database exists check failure"));
		}

		Ok(self.commitments.read().await.contains_key(hash))
	}

	/// Insert a signed delegation into the in-memory store for a given slot for testing.
	///
	/// Adds the provided `delegation` to the vector associated with `slot`, creating the vector if none exists.
	///
	/// # Examples
	///
	/// ```ignore
	/// use crate::testing::mocks::MockConstraintsApiClient;
	/// use crate::types::delegation::SignedDelegation;
	///
	/// #[tokio::test]
	/// async fn example_add_delegation() {
	///     let client = MockConstraintsApiClient::new();
	///     // Construct a test delegation; replace with a real SignedDelegation as needed.
	///     let delegation = SignedDelegation::default();
	///     client.add_delegation(42, delegation).await;
	///     let delegations = client.mock_get_delegations_for_slot(42).await.unwrap();
	///     assert_eq!(delegations.len(), 1);
	/// }
	/// ```ignore
	pub async fn add_delegation(&self, slot: u64, delegation: SignedDelegation) {
		self.delegations.write().await.entry(slot).or_insert_with(Vec::new).push(delegation);
	}

	/// Fetches the signed delegations stored for a specific slot from the mock database.
	///
	/// Returns an owned vector of `SignedDelegation` entries associated with `slot`.
	/// If no delegations exist for the slot, an empty vector is returned.
	/// Returns an error if the mock database is configured to fail.
	///
	/// # Examples
	///
	/// ```ignore
	/// use crate::testing::mocks::MockDatabase;
	///
	/// let rt = tokio::runtime::Runtime::new().unwrap();
	/// rt.block_on(async {
	///     let db = MockDatabase::new();
	///     let delegations = db.mock_get_delegations_for_slot(42).await.unwrap();
	///     assert!(delegations.is_empty());
	/// });
	/// ```ignore
	pub async fn mock_get_delegations_for_slot(&self, slot: u64) -> Result<Vec<SignedDelegation>> {
		if self.latency_ms > 0 {
			tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
		}

		if self.should_fail {
			return Err(anyhow::anyhow!("Mock database delegation query failure"));
		}

		Ok(self.delegations.read().await.get(&slot).cloned().unwrap_or_default())
	}

	/// Returns a clone of all stored commitments keyed by request hash.
	///
	/// The returned map contains every commitment currently held in the mock database.
	///
	/// # Examples
	///
	/// ```ignore
	/// let db = crate::testing::mocks::MockDatabase::new();
	/// let rt = tokio::runtime::Runtime::new().unwrap();
	/// let all = rt.block_on(db.get_all_commitments());
	/// assert!(all.is_empty());
	/// ```ignore
	pub async fn get_all_commitments(&self) -> HashMap<String, crate::types::SignedCommitment> {
		self.commitments.read().await.clone()
	}

	/// Clears all stored commitments and delegations from the mock database.
	///
	/// # Examples
	///
	/// ```ignore
	/// let rt = tokio::runtime::Runtime::new().unwrap();
	/// rt.block_on(async {
	///     let db = MockDatabase::new();
	///     // populate db...
	///     db.clear_all().await;
	/// });
	/// ```ignore
	pub async fn clear_all(&self) {
		self.commitments.write().await.clear();
		self.delegations.write().await.clear();
	}
}

/// Build a Config populated with test-oriented values and mock endpoints for use in unit tests.
///
/// The returned `Config` sets deterministic, short-lived values (localhost endpoints, short timeouts,
/// debug logging, and test addresses) suitable for isolated test environments.
///
/// # Examples
///
/// ```ignore
/// let cfg = create_test_config();
/// assert_eq!(cfg.server.host, "127.0.0.1");
/// assert_eq!(cfg.beacon_api.primary_endpoint, "http://localhost:5051");
/// ```ignore
pub fn create_test_config() -> Config {
	Config {
		server: crate::config::ServerConfig {
			host: "127.0.0.1".to_string(),
			port: 0, // Random port for testing
		},
		database: crate::config::DatabaseConfig { url: "postgresql://test:test@localhost/test_db".to_string() },
		logging: crate::config::LoggingConfig {
			level: "debug".to_string(),
			enable_method_tracing: true,
			traced_methods: vec![],
		},
		validation: crate::config::ValidationConfig {
			slasher_address: "0x1234567890123456789012345678901234567890".to_string(),
		},
		beacon_api: crate::config::BeaconApiConfig {
			primary_endpoint: "http://localhost:5051".to_string(),
			fallback_endpoints: vec!["http://localhost:5052".to_string()],
			request_timeout_secs: 1,
			genesis_time: 1606824023, // Eth2 mainnet genesis
		},
		constraints_api: crate::config::ConstraintsApiConfig {
			relay_endpoint: "http://localhost:8080".to_string(),
			request_timeout_secs: 1,
			max_retries: 1,
			authorized_builders: vec![],
		},
		delegation: crate::config::DelegationConfig {
			lookahead_epochs: 2,
			polling_interval_secs: 60,
			cache_ttl_secs: 300,
			domain_application_gateway: "0x00000002".to_string(),
		},
		reth: crate::config::RethConfig::default(),
		signing: crate::config::SigningConfig::default(),
	}
}

/// Generate a test BLS key pair for use in tests.
///
/// Returns a freshly generated BLS secret key and its corresponding public key wrapped in
/// `crate::types::delegation::BlsPublicKey`.
///
/// # Examples
///
/// ```ignore
/// let (secret_key, public_key) = create_test_bls_keypair();
/// // secret_key is a `blst::min_pk::SecretKey` and public_key wraps the public key bytes
/// assert!(!public_key.0.is_empty());
/// ```ignore
pub fn create_test_bls_keypair() -> (blst::min_pk::SecretKey, crate::types::delegation::BlsPublicKey) {
	use blst::min_pk::SecretKey;
	let mut rng = rand::thread_rng();
	let ikm: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
	let secret_key = SecretKey::key_gen(&ikm, &[]).unwrap();
	let public_key = secret_key.sk_to_pk();
	let public_key_wrapper = crate::types::delegation::BlsPublicKey(public_key.to_bytes());
	(secret_key, public_key_wrapper)
}

/// Create a deterministic test ECDSA key pair and its Ethereum address.
///
/// The returned tuple contains the secp256k1 secret key and the corresponding
/// 0x-prefixed Ethereum address derived from the uncompressed public key.
///
/// # Examples
///
/// ```ignore
/// let (sk, addr) = create_test_ecdsa_keypair();
/// // secret key is a 32-byte secp256k1 key
/// assert_eq!(sk[..].len(), 32);
/// // address is a 0x-prefixed 20-byte (40 hex chars) Ethereum address
/// assert!(addr.starts_with("0x"));
/// assert_eq!(addr.len(), 42);
/// ```ignore
pub fn create_test_ecdsa_keypair() -> (secp256k1::SecretKey, String) {
	let secret_key = secp256k1::SecretKey::from_slice(&[2u8; 32]).unwrap();
	let public_key = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &secret_key);

	// Derive Ethereum address from public key
	let public_key_bytes = public_key.serialize_uncompressed();
	let hash = crate::crypto::keccak256(&public_key_bytes[1..]);
	let address = format!("0x{}", hex::encode(&hash[12..]));

	(secret_key, address)
}
