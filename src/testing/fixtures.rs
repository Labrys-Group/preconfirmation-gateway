use crate::testing::mocks::{create_test_bls_keypair, create_test_ecdsa_keypair};
use crate::types::{
	CommitmentRequest,
	delegation::{BlsPublicKey, BlsSignature, DelegationMessage, SignedDelegation},
	payload::{InclusionPayload, PayloadParser},
};
use std::collections::HashMap;

/// Test fixtures for various scenarios
pub struct TestFixtures;

impl TestFixtures {
	/// Constructs a CommitmentRequest containing an inclusion payload for the given slot and committer address.
	///
	/// The returned request uses commitment_type 1 (Inclusion preconfirmation) and contains an encoded inclusion payload for testing.
	///
	/// # Examples
	///
	/// ```
	/// let req = create_inclusion_commitment_request(42, "committer-addr");
	/// assert_eq!(req.commitment_type, 1);
	/// assert_eq!(req.slasher, "committer-addr".to_string());
	/// assert!(!req.payload.is_empty());
	/// ```
	pub fn create_inclusion_commitment_request(slot: u64, committer_address: &str) -> CommitmentRequest {
		// Create a simple inclusion payload
		let inclusion_payload = InclusionPayload::new(
			slot,
			vec![0x01, 0x02, 0x03, 0x04], // Simple transaction data
		);

		// Encode the payload as JSON (simplest format for testing)
		let payload_bytes =
			PayloadParser::encode_inclusion_payload(&inclusion_payload).expect("Failed to encode test payload");

		CommitmentRequest {
			commitment_type: 1, // Inclusion preconfirmation
			payload: payload_bytes,
			slasher: committer_address.to_string(),
		}
	}

	/// Constructs a SignedDelegation for tests using the provided proposer and delegate keys and slot.
	///
	/// The returned SignedDelegation contains the given proposer, delegate, committer address, and slot,
	/// along with a deterministic mock 96-byte BLS signature used for testing purposes.
	///
	/// # Examples
	///
	/// ```
	/// let proposer = BlsPublicKey([1u8; 48]);
	/// let delegate = BlsPublicKey([2u8; 48]);
	/// let signed = create_signed_delegation(42, proposer, delegate, "committer@example.com");
	/// assert_eq!(signed.message.slot, 42);
	/// assert_eq!(signed.message.committer, "committer@example.com".to_string());
	/// ```
	pub fn create_signed_delegation(
		slot: u64,
		proposer_key: BlsPublicKey,
		delegate_key: BlsPublicKey,
		committer_address: &str,
	) -> SignedDelegation {
		let delegation_message = DelegationMessage {
			proposer: proposer_key,
			delegate: delegate_key,
			committer: committer_address.to_string(),
			slot,
		};

		// Create a mock signature (in real scenarios, this would be signed by the proposer)
		let mock_signature = BlsSignature([42u8; 96]); // Mock signature

		SignedDelegation { message: delegation_message, signature: mock_signature }
	}

	/// Generate a set of sample SignedDelegation entries for testing.
	///
	/// The returned map contains three entries keyed as:
	/// - "delegation1": a normal delegation (proposer1 -> delegate1) with committer1
	/// - "delegation2": a different proposer with the same delegate as delegation1 (proposer2 -> delegate1) with committer1
	/// - "delegation3": a delegation with the same proposer as delegation1 but a different delegate and a different committer (proposer1 -> delegate2) with committer2
	///
	/// Each SignedDelegation is constructed for the provided slot and uses test keypairs.
	///
	/// # Examples
	///
	/// ```
	/// let delegations = TestFixtures::create_test_delegations(100);
	/// assert!(delegations.contains_key("delegation1"));
	/// assert!(delegations.contains_key("delegation2"));
	/// assert!(delegations.contains_key("delegation3"));
	/// ```
	pub fn create_test_delegations(slot: u64) -> HashMap<String, SignedDelegation> {
		let mut delegations = HashMap::new();

		// Create test key pairs
		let (_proposer_sk1, proposer_pk1) = create_test_bls_keypair();
		let (_proposer_sk2, proposer_pk2) = create_test_bls_keypair();
		let (_delegate_sk1, delegate_pk1) = create_test_bls_keypair();
		let (_delegate_sk2, delegate_pk2) = create_test_bls_keypair();

		let (_ecdsa_sk1, committer1) = create_test_ecdsa_keypair();
		let (_ecdsa_sk2, committer2) = create_test_ecdsa_keypair();

		// Delegation 1: Normal delegation
		let delegation1 = Self::create_signed_delegation(slot, proposer_pk1, delegate_pk1, &committer1);
		delegations.insert("delegation1".to_string(), delegation1);

		// Delegation 2: Different proposer, same delegate
		let delegation2 = Self::create_signed_delegation(slot, proposer_pk2, delegate_pk1, &committer1);
		delegations.insert("delegation2".to_string(), delegation2);

		// Delegation 3: Different committer
		let delegation3 = Self::create_signed_delegation(slot, proposer_pk1, delegate_pk2, &committer2);
		delegations.insert("delegation3".to_string(), delegation3);

		delegations
	}

	/// Builds a set of predefined TestScenario entries covering common delegation and commitment cases.
	///
	/// The returned map uses string keys identifying each scenario:
	/// - "happy_path": valid delegation and commitment
	/// - "no_delegation": no delegation available for the slot
	/// - "invalid_payload": malformed commitment payload
	/// - "wrong_commitment_type": unsupported commitment type
	/// - "duplicate_commitment": duplicate commitment request
	///
	/// Returns a HashMap mapping scenario names to their TestScenario definitions.
	///
	/// # Examples
	///
	/// ```
	/// let scenarios = create_test_scenarios();
	/// assert!(scenarios.contains_key("happy_path"));
	/// assert_eq!(scenarios["happy_path"].expected_success, true);
	/// ```
	pub fn create_test_scenarios() -> HashMap<String, TestScenario> {
		let mut scenarios = HashMap::new();

		// Use current slot numbers for realistic testing
		let current_slot = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 12;

		// Scenario 1: Happy path - valid delegation and commitment
		scenarios.insert(
			"happy_path".to_string(),
			TestScenario {
				name: "Happy Path".to_string(),
				description: "Valid delegation and commitment request".to_string(),
				slot: current_slot,
				has_delegation: true,
				delegation_valid: true,
				commitment_valid: true,
				expected_success: true,
				expected_error: None,
			},
		);

		// Scenario 2: No delegation found (use a slot that will trigger no delegation)
		let no_delegation_slot = (current_slot / 10) * 10 + 1; // Ensure slot % 10 == 1
		scenarios.insert(
			"no_delegation".to_string(),
			TestScenario {
				name: "No Delegation".to_string(),
				description: "Commitment request without valid delegation".to_string(),
				slot: no_delegation_slot,
				has_delegation: false,
				delegation_valid: false,
				commitment_valid: true,
				expected_success: false,
				expected_error: Some("No valid delegation found".to_string()),
			},
		);

		// Scenario 3: Invalid payload
		scenarios.insert(
			"invalid_payload".to_string(),
			TestScenario {
				name: "Invalid Payload".to_string(),
				description: "Commitment request with malformed payload".to_string(),
				slot: current_slot + 2,
				has_delegation: true,
				delegation_valid: true,
				commitment_valid: false,
				expected_success: false,
				expected_error: Some("Failed to extract slot from payload".to_string()),
			},
		);

		// Scenario 4: Wrong commitment type
		scenarios.insert(
			"wrong_commitment_type".to_string(),
			TestScenario {
				name: "Wrong Commitment Type".to_string(),
				description: "Commitment request with unsupported type".to_string(),
				slot: current_slot + 3,
				has_delegation: true,
				delegation_valid: true,
				commitment_valid: true,
				expected_success: false,
				expected_error: Some("Invalid commitment type".to_string()),
			},
		);

		// Scenario 5: Duplicate commitment
		scenarios.insert(
			"duplicate_commitment".to_string(),
			TestScenario {
				name: "Duplicate Commitment".to_string(),
				description: "Duplicate commitment request".to_string(),
				slot: current_slot + 4,
				has_delegation: true,
				delegation_valid: true,
				commitment_valid: true,
				expected_success: false,
				expected_error: Some("Duplicate commitment request".to_string()),
			},
		);

		scenarios
	}

	/// Creates an encoded inclusion payload for tests containing the given slot and fixed sample transaction bytes.
	///
	/// The returned byte vector is the result of encoding an InclusionPayload with the provided `slot` and a small
	/// hard-coded test transaction payload.
	///
	/// # Returns
	///
	/// A `Vec<u8>` containing the encoded inclusion payload.
	///
	/// # Examples
	///
	/// ```
	/// let bytes = crate::testing::fixtures::create_test_payload(42);
	/// assert!(!bytes.is_empty());
	/// ```
	pub fn create_test_payload(slot: u64) -> Vec<u8> {
		let inclusion_payload = InclusionPayload::new(
			slot,
			vec![0xde, 0xad, 0xbe, 0xef], // Test transaction data
		);

		PayloadParser::encode_inclusion_payload(&inclusion_payload).expect("Failed to encode test payload")
	}

	/// Constructs a deliberately malformed payload for testing error handling.
	///
	/// The returned byte vector does not conform to any valid payload format and is intended to trigger
	/// parse or validation errors in tests.
	///
	/// # Examples
	///
	/// ```
	/// let payload = create_invalid_payload();
	/// assert_eq!(payload, vec![0xff, 0xff, 0xff, 0xff]);
	/// ```
	pub fn create_invalid_payload() -> Vec<u8> {
		vec![0xff, 0xff, 0xff, 0xff] // Invalid payload that can't be parsed
	}

	/// Create a CommitmentRequest populated with the given type, payload, and slasher address.
	///
	/// # Examples
	///
	/// ```
	/// let payload = vec![1, 2, 3];
	/// let req = create_commitment_request(1, payload.clone(), "0xabc");
	/// assert_eq!(req.commitment_type, 1);
	/// assert_eq!(req.payload, payload);
	/// assert_eq!(req.slasher, "0xabc".to_string());
	/// ```
	pub fn create_commitment_request(commitment_type: u64, payload: Vec<u8>, slasher: &str) -> CommitmentRequest {
		CommitmentRequest { commitment_type, payload, slasher: slasher.to_string() }
	}
}

/// Test scenario definition
#[derive(Debug, Clone)]
pub struct TestScenario {
	pub name: String,
	pub description: String,
	pub slot: u64,
	pub has_delegation: bool,
	pub delegation_valid: bool,
	pub commitment_valid: bool,
	pub expected_success: bool,
	pub expected_error: Option<String>,
}

/// Performance test configuration
#[derive(Debug, Clone)]
pub struct PerformanceTestConfig {
	pub name: String,
	pub description: String,
	pub concurrent_requests: usize,
	pub total_requests: usize,
	pub max_duration_ms: u64,
	pub expected_tps: f64, // Transactions per second
}

impl PerformanceTestConfig {
	/// Provides three preset performance test configurations for common load scenarios.
	///
	/// The presets are:
	/// - `light_load`: 10 concurrent, 100 total, 5_000 ms max duration, ~20.0 TPS
	/// - `medium_load`: 50 concurrent, 500 total, 10_000 ms max duration, ~50.0 TPS
	/// - `high_load`: 100 concurrent, 1_000 total, 20_000 ms max duration, ~50.0 TPS (may be lower under high load)
	///
	/// # Returns
	///
	/// A `Vec<PerformanceTestConfig>` containing the three preset configurations in the order: light, medium, high.
	///
	/// # Examples
	///
	/// ```
	/// let configs = PerformanceTestConfig::default_configs();
	/// assert_eq!(configs.len(), 3);
	/// assert_eq!(configs[0].name, "light_load");
	/// ```
	pub fn default_configs() -> Vec<PerformanceTestConfig> {
		vec![
			PerformanceTestConfig {
				name: "light_load".to_string(),
				description: "Light load test with 10 concurrent requests".to_string(),
				concurrent_requests: 10,
				total_requests: 100,
				max_duration_ms: 5000,
				expected_tps: 20.0,
			},
			PerformanceTestConfig {
				name: "medium_load".to_string(),
				description: "Medium load test with 50 concurrent requests".to_string(),
				concurrent_requests: 50,
				total_requests: 500,
				max_duration_ms: 10000,
				expected_tps: 50.0,
			},
			PerformanceTestConfig {
				name: "high_load".to_string(),
				description: "High load test with 100 concurrent requests".to_string(),
				concurrent_requests: 100,
				total_requests: 1000,
				max_duration_ms: 20000,
				expected_tps: 50.0, // May be lower under high load
			},
		]
	}
}

/// Timing test helpers
pub struct TimingTestHelpers;

impl TimingTestHelpers {
	/// Produces a set of slots around the current slot to exercise submission timing windows.
	///
	/// Returns five slot numbers covering: a far past slot, a recent past slot, the current slot,
	/// a near future slot, and a far future slot. These are useful for testing acceptance/rejection
	/// based on timing constraints.
	///
	/// # Examples
	///
	/// ```
	/// let slots = create_timing_test_slots();
	/// // Expect five slots in increasing order: far past, recent past, current, near future, far future
	/// assert_eq!(slots.len(), 5);
	/// assert!(slots[0] < slots[1] && slots[1] < slots[2] && slots[2] < slots[3] && slots[3] < slots[4]);
	/// ```
	pub fn create_timing_test_slots() -> Vec<u64> {
		let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		let current_slot = current_time / 12;

		// Create slots that are in different timing windows
		vec![
			current_slot - 50, // Far past slot (should be rejected)
			current_slot - 5,  // Recent past slot (should be rejected)
			current_slot,      // Current slot (should be accepted)
			current_slot + 5,  // Near future slot (should be accepted)
			current_slot + 50, // Far future slot (should be rejected)
		]
	}

	/// Determines whether a slot is within the 10-slot submission window relative to the current time.
	///
	/// The function computes the current slot from system time and returns whether the absolute
	/// difference between the provided `slot` and that current slot is less than or equal to 10.
	///
	/// # Parameters
	///
	/// - `_genesis_time`: currently unused; retained for API compatibility.
	/// - `slot`: the slot to evaluate.
	///
	/// # Returns
	///
	/// `true` if the absolute difference between `slot` and the current slot is less than or equal to 10, `false` otherwise.
	///
	/// # Examples
	///
	/// ```
	/// let near_future_slot = (std::time::SystemTime::now()
	///     .duration_since(std::time::UNIX_EPOCH)
	///     .unwrap()
	///     .as_secs() / 12) + 5;
	/// assert!(is_within_submission_window(0, near_future_slot));
	/// ```
	pub fn is_within_submission_window(_genesis_time: u64, slot: u64) -> bool {
		let current_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		let current_slot = current_time / 12;
		let slot_diff = if slot > current_slot { slot - current_slot } else { current_slot - slot };

		// Allow slots within 10 slots of current (reasonable constraint submission window)
		slot_diff <= 10
	}
}
