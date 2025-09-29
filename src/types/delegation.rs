use serde::{Deserialize, Serialize, Deserializer, Serializer};

/// BLS Public Key representation (48 bytes)
/// Custom serialization to handle byte arrays properly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlsPublicKey(pub [u8; 48]);

/// BLS Signature representation (96 bytes)
/// Custom serialization to handle byte arrays properly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlsSignature(pub [u8; 96]);

impl Serialize for BlsPublicKey {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(&format!("0x{}", hex::encode(self.0)))
	}
}

impl<'de> Deserialize<'de> for BlsPublicKey {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let hex_str: String = String::deserialize(deserializer)?;
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
		let bytes = hex::decode(hex_str).map_err(serde::de::Error::custom)?;

		if bytes.len() != 48 {
			return Err(serde::de::Error::custom(format!(
				"Expected 48 bytes for BLS public key, got {}",
				bytes.len()
			)));
		}

		let mut key = [0u8; 48];
		key.copy_from_slice(&bytes);
		Ok(BlsPublicKey(key))
	}
}

impl Serialize for BlsSignature {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(&format!("0x{}", hex::encode(self.0)))
	}
}

impl<'de> Deserialize<'de> for BlsSignature {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let hex_str: String = String::deserialize(deserializer)?;
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
		let bytes = hex::decode(hex_str).map_err(serde::de::Error::custom)?;

		if bytes.len() != 96 {
			return Err(serde::de::Error::custom(format!(
				"Expected 96 bytes for BLS signature, got {}",
				bytes.len()
			)));
		}

		let mut sig = [0u8; 96];
		sig.copy_from_slice(&bytes);
		Ok(BlsSignature(sig))
	}
}

// Conversion traits for easier usage
impl From<[u8; 48]> for BlsPublicKey {
	fn from(bytes: [u8; 48]) -> Self {
		BlsPublicKey(bytes)
	}
}

impl From<BlsPublicKey> for [u8; 48] {
	fn from(key: BlsPublicKey) -> Self {
		key.0
	}
}

impl AsRef<[u8; 48]> for BlsPublicKey {
	fn as_ref(&self) -> &[u8; 48] {
		&self.0
	}
}

impl From<[u8; 96]> for BlsSignature {
	fn from(bytes: [u8; 96]) -> Self {
		BlsSignature(bytes)
	}
}

impl From<BlsSignature> for [u8; 96] {
	fn from(sig: BlsSignature) -> Self {
		sig.0
	}
}

impl AsRef<[u8; 96]> for BlsSignature {
	fn as_ref(&self) -> &[u8; 96] {
		&self.0
	}
}

/// Core delegation message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationMessage {
	/// BLS Public Key of the scheduled validator (the authority)
	pub proposer: BlsPublicKey,
	/// BLS Public Key of the Gateway (the recipient of the authority)
	pub delegate: BlsPublicKey,
	/// ECDSA execution layer address the Gateway used to sign commitments
	pub committer: String, // Ethereum address as hex string
	/// The specific slot number this delegation applies to
	pub slot: u64,
}

/// A delegation message with its BLS signature from the proposer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedDelegation {
	pub message: DelegationMessage,
	/// BLS signature by the proposer over the delegation message
	pub signature: BlsSignature,
}

/// Constraint instruction for block builders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
	/// Unique identifier determining how payload should be interpreted
	/// For Inclusion Preconfirmation: 0x01
	pub constraint_type: u64,
	/// Opaque byte array containing constraint-specific data
	/// For Inclusion Preconfs: reused directly from CommitmentRequest payload
	pub payload: Vec<u8>,
}

/// Container for multiple constraints targeted at a specific slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintsMessage {
	/// BLS Public Key of the scheduled proposer
	pub proposer: BlsPublicKey,
	/// BLS Public Key of the Gateway (delegate)
	pub delegate: BlsPublicKey,
	/// Target L1 slot number
	pub slot: u64,
	/// List of constraints to be processed in order
	pub constraints: Vec<Constraint>,
	/// List of Builder BLS public keys authorized to access constraints
	/// Empty list means publicly accessible
	pub receivers: Vec<BlsPublicKey>,
}

/// Signed constraints message for submission to relay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedConstraints {
	pub message: ConstraintsMessage,
	/// BLS signature by the Gateway's delegate key
	pub signature: BlsSignature,
}

/// Proposer duty information from Beacon API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposerDuty {
	/// Validator index
	pub validator_index: u64,
	/// BLS public key of the validator
	pub pubkey: BlsPublicKey,
	/// Slot number the validator is scheduled to propose
	pub slot: u64,
}

/// Response from Beacon API for proposer duties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposerDutyResponse {
	pub data: Vec<ProposerDuty>,
}

/// Domain separation constants for BLS signatures
pub mod domains {
	/// Domain separator for delegation signatures (from spec: 0x0044656c)
	pub const DELEGATION_DOMAIN_SEPARATOR: [u8; 4] = [0x00, 0x44, 0x65, 0x6c];

	/// Domain separator for constraint signatures (configurable, spec shows TBD)
	/// This will be loaded from configuration
	pub fn application_gateway_domain() -> [u8; 4] {
		// Default value, should be overridden by configuration
		[0x00, 0x00, 0x00, 0x02]
	}
}

/// Helper functions for delegation processing
impl SignedDelegation {
	/// Check if this delegation is valid for the given slot
	pub fn is_valid_for_slot(&self, slot: u64) -> bool {
		self.message.slot == slot
	}

	/// Get the committer address that should be used for commitment signing
	pub fn get_committer_address(&self) -> &str {
		&self.message.committer
	}

	/// Get the delegate BLS public key for constraint signing
	pub fn get_delegate_key(&self) -> &BlsPublicKey {
		&self.message.delegate
	}

	/// Get the proposer BLS public key for signature verification
	pub fn get_proposer_key(&self) -> &BlsPublicKey {
		&self.message.proposer
	}

	/// Get the delegate BLS public key as bytes
	pub fn get_delegate_bytes(&self) -> &[u8; 48] {
		&self.message.delegate.0
	}

	/// Get the proposer BLS public key as bytes
	pub fn get_proposer_bytes(&self) -> &[u8; 48] {
		&self.message.proposer.0
	}

	/// Get the signature as bytes
	pub fn get_signature_bytes(&self) -> &[u8; 96] {
		&self.signature.0
	}
}

impl ConstraintsMessage {
	/// Create a new constraints message for a slot
	pub fn new(
		proposer: BlsPublicKey,
		delegate: BlsPublicKey,
		slot: u64,
		constraints: Vec<Constraint>,
		receivers: Vec<BlsPublicKey>,
	) -> Self {
		Self {
			proposer,
			delegate,
			slot,
			constraints,
			receivers,
		}
	}

	/// Add a constraint to the message
	pub fn add_constraint(&mut self, constraint: Constraint) {
		self.constraints.push(constraint);
	}
}

impl Constraint {
	/// Create a new inclusion constraint from a commitment payload
	pub fn from_inclusion_commitment(payload: Vec<u8>) -> Self {
		Self {
			constraint_type: 1, // Inclusion Preconfirmation
			payload,
		}
	}
}