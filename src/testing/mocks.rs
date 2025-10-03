use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use anyhow::Result;
use tokio::sync::RwLock;
use rand::Rng;

use crate::api::beacon::BeaconApiClient;
use crate::api::constraints::{ConstraintsApiClient, ConstraintSubmissionResponse};
use crate::config::Config;
use crate::types::beacon::ProposerDutiesResponse;
use crate::types::delegation::{SignedDelegation, SignedConstraints};

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
    fn default() -> Self {
        Self::new()
    }
}

impl MockBeaconApiClient {
    pub fn new() -> Self {
        Self {
            proposer_duties: Arc::new(RwLock::new(HashMap::new())),
            network_delay_ms: 50,
            should_fail: false,
        }
    }

    /// Add mock proposer duties for an epoch
    pub async fn add_proposer_duties(&self, epoch: u64, duties: ProposerDutiesResponse) {
        self.proposer_duties.write().await.insert(epoch, duties);
    }

    /// Set network delay for testing
    pub fn set_network_delay(&mut self, delay_ms: u64) {
        self.network_delay_ms = delay_ms;
    }

    /// Set failure mode for testing error handling
    pub fn set_failure_mode(&mut self, should_fail: bool) {
        self.should_fail = should_fail;
    }
}

impl BeaconApiClient {
    /// Create mock client that behaves like the real client but with controllable responses
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
    fn default() -> Self {
        Self::new()
    }
}

impl MockConstraintsApiClient {
    pub fn new() -> Self {
        Self {
            delegations: Arc::new(RwLock::new(HashMap::new())),
            submitted_constraints: Arc::new(Mutex::new(Vec::new())),
            network_delay_ms: 100,
            should_fail: false,
            submission_responses: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add mock delegation for a slot
    pub async fn add_delegation(&self, slot: u64, delegation: SignedDelegation) {
        self.delegations.write().await
            .entry(slot)
            .or_insert_with(Vec::new)
            .push(delegation);
    }

    /// Get all submitted constraints (for testing verification)
    pub fn get_submitted_constraints(&self) -> Vec<SignedConstraints> {
        self.submitted_constraints.lock().unwrap().clone()
    }

    /// Clear submitted constraints log
    pub fn clear_submitted_constraints(&self) {
        self.submitted_constraints.lock().unwrap().clear();
    }

    /// Set network delay for testing
    pub fn set_network_delay(&mut self, delay_ms: u64) {
        self.network_delay_ms = delay_ms;
    }

    /// Set failure mode
    pub fn set_failure_mode(&mut self, should_fail: bool) {
        self.should_fail = should_fail;
    }

    /// Add mock response for constraint submission
    pub async fn add_submission_response(&self, response: ConstraintSubmissionResponse) {
        self.submission_responses.write().await.push(response);
    }

    /// Simulate constraint submission (captures the constraints for verification)
    pub async fn mock_submit_constraints(&self, constraints: &SignedConstraints) -> Result<ConstraintSubmissionResponse> {
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
            Ok(ConstraintSubmissionResponse {
                success: true,
                submission_id: Some("test_submission_id".to_string()),
            })
        }
    }

    /// Mock get delegations for slot
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
    /// Create mock client
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
    fn default() -> Self {
        Self::new()
    }
}

impl MockDatabase {
    pub fn new() -> Self {
        Self {
            commitments: Arc::new(RwLock::new(HashMap::new())),
            delegations: Arc::new(RwLock::new(HashMap::new())),
            latency_ms: 10,
            should_fail: false,
        }
    }

    /// Set database latency for testing
    pub fn set_latency(&mut self, latency_ms: u64) {
        self.latency_ms = latency_ms;
    }

    /// Set failure mode
    pub fn set_failure_mode(&mut self, should_fail: bool) {
        self.should_fail = should_fail;
    }

    /// Mock save commitment
    pub async fn mock_save_commitment(&self, commitment: &crate::types::SignedCommitment) -> Result<()> {
        if self.latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
        }

        if self.should_fail {
            return Err(anyhow::anyhow!("Mock database save failure"));
        }

        self.commitments.write().await.insert(
            commitment.commitment.request_hash.clone(),
            commitment.clone()
        );
        Ok(())
    }

    /// Mock get commitment by hash
    pub async fn mock_get_commitment(&self, hash: &str) -> Result<Option<crate::types::SignedCommitment>> {
        if self.latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
        }

        if self.should_fail {
            return Err(anyhow::anyhow!("Mock database get failure"));
        }

        Ok(self.commitments.read().await.get(hash).cloned())
    }

    /// Mock commitment exists check
    pub async fn mock_commitment_exists(&self, hash: &str) -> Result<bool> {
        if self.latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
        }

        if self.should_fail {
            return Err(anyhow::anyhow!("Mock database exists check failure"));
        }

        Ok(self.commitments.read().await.contains_key(hash))
    }

    /// Add delegation for testing
    pub async fn add_delegation(&self, slot: u64, delegation: SignedDelegation) {
        self.delegations.write().await
            .entry(slot)
            .or_insert_with(Vec::new)
            .push(delegation);
    }

    /// Mock get delegations for slot
    pub async fn mock_get_delegations_for_slot(&self, slot: u64) -> Result<Vec<SignedDelegation>> {
        if self.latency_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.latency_ms)).await;
        }

        if self.should_fail {
            return Err(anyhow::anyhow!("Mock database delegation query failure"));
        }

        Ok(self.delegations.read().await.get(&slot).cloned().unwrap_or_default())
    }

    /// Get all stored commitments (for testing verification)
    pub async fn get_all_commitments(&self) -> HashMap<String, crate::types::SignedCommitment> {
        self.commitments.read().await.clone()
    }

    /// Clear all data
    pub async fn clear_all(&self) {
        self.commitments.write().await.clear();
        self.delegations.write().await.clear();
    }
}

/// Create a test configuration with mock endpoints
pub fn create_test_config() -> Config {
    Config {
        server: crate::config::ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 0, // Random port for testing
        },
        database: crate::config::DatabaseConfig {
            url: "postgresql://test:test@localhost/test_db".to_string(),
        },
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

/// Create test BLS key pair
pub fn create_test_bls_keypair() -> (blst::min_pk::SecretKey, crate::types::delegation::BlsPublicKey) {
    use blst::min_pk::SecretKey;
    let mut rng = rand::thread_rng();
    let ikm: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    let secret_key = SecretKey::key_gen(&ikm, &[]).unwrap();
    let public_key = secret_key.sk_to_pk();
    let public_key_wrapper = crate::types::delegation::BlsPublicKey(public_key.to_bytes());
    (secret_key, public_key_wrapper)
}

/// Create test ECDSA key pair
pub fn create_test_ecdsa_keypair() -> (secp256k1::SecretKey, String) {
    let secret_key = secp256k1::SecretKey::from_slice(&[2u8; 32]).unwrap();
    let public_key = secp256k1::PublicKey::from_secret_key(&secp256k1::Secp256k1::new(), &secret_key);

    // Derive Ethereum address from public key
    let public_key_bytes = public_key.serialize_uncompressed();
    let hash = crate::crypto::keccak256(&public_key_bytes[1..]);
    let address = format!("0x{}", hex::encode(&hash[12..]));

    (secret_key, address)
}