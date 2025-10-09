use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// BLS Public Key representation (48 bytes)
/// Custom serialization to handle byte arrays properly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlsPublicKey(pub [u8; 48]);

/// BLS Signature representation (96 bytes)
/// Custom serialization to handle byte arrays properly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlsSignature(pub [u8; 96]);

impl Serialize for BlsPublicKey {
	/// Serializes the key as a hex string prefixed with `0x`.
	///
	/// The output is a JSON string containing the lowercase hex encoding of the inner 48-byte array with a `0x` prefix.
	///
	/// # Examples
	///
	/// ```ignore
	/// use serde::Serialize;
	/// // construct a BlsPublicKey from bytes (example uses zeros)
	/// let key = crate::types::delegation::BlsPublicKey([0u8; 48]);
	/// let json = serde_json::to_string(&key).unwrap();
	/// // serialized string is quoted JSON string that begins with "0x"
	/// assert!(json.starts_with("\"0x"));
	/// ```ignore
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(&format!("0x{}", hex::encode(self.0)))
	}
}

impl<'de> Deserialize<'de> for BlsPublicKey {
	/// Deserialize a BLS public key from a hex-encoded string, accepting an optional `0x` prefix.
	///
	/// The deserializer expects a hex string that decodes to exactly 48 bytes and returns an
	/// instance of `BlsPublicKey`. Deserialization fails if the string is not valid hex or if the
	/// decoded byte length is not 48.
	///
	/// # Examples
	///
	/// ```ignore
	/// // Produces a JSON string containing a 48-byte all-zero public key with `0x` prefix.
	/// let json = format!("\"0x{}\"", "00".repeat(48));
	/// let pk: BlsPublicKey = serde_json::from_str(&json).unwrap();
	/// assert_eq!(pk.0, [0u8; 48]);
	/// ```ignore
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let hex_str: String = String::deserialize(deserializer)?;
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
		let bytes = hex::decode(hex_str).map_err(serde::de::Error::custom)?;

		if bytes.len() != 48 {
			return Err(serde::de::Error::custom(format!("Expected 48 bytes for BLS public key, got {}", bytes.len())));
		}

		let mut key = [0u8; 48];
		key.copy_from_slice(&bytes);
		Ok(BlsPublicKey(key))
	}
}

impl Serialize for BlsSignature {
	/// Serializes the key as a hex string prefixed with `0x`.
	///
	/// The output is a JSON string containing the lowercase hex encoding of the inner 48-byte array with a `0x` prefix.
	///
	/// # Examples
	///
	/// ```ignore
	/// use serde::Serialize;
	/// // construct a BlsPublicKey from bytes (example uses zeros)
	/// let key = crate::types::delegation::BlsPublicKey([0u8; 48]);
	/// let json = serde_json::to_string(&key).unwrap();
	/// // serialized string is quoted JSON string that begins with "0x"
	/// assert!(json.starts_with("\"0x"));
	/// ```ignore
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(&format!("0x{}", hex::encode(self.0)))
	}
}

impl<'de> Deserialize<'de> for BlsSignature {
	/// Deserializes a `BlsSignature` from a hex string (optionally prefixed with `0x`).
	///
	/// Expects the input to be a hex-encoded string representing exactly 96 bytes; returns an
	/// error if the hex is invalid or does not decode to 96 bytes. The resulting `BlsSignature`
	/// contains the decoded 96-byte array.
	///
	/// # Examples
	///
	/// ```ignore
	/// use serde_json;
	/// use crate::types::delegation::BlsSignature;
	///
	/// let json = format!("\"0x{}\"", "00".repeat(96)); // 96 bytes of zeros
	/// let sig: BlsSignature = serde_json::from_str(&json).unwrap();
	/// assert_eq!(sig.as_ref().len(), 96);
	/// ```ignore
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let hex_str: String = String::deserialize(deserializer)?;
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(&hex_str);
		let bytes = hex::decode(hex_str).map_err(serde::de::Error::custom)?;

		if bytes.len() != 96 {
			return Err(serde::de::Error::custom(format!("Expected 96 bytes for BLS signature, got {}", bytes.len())));
		}

		let mut sig = [0u8; 96];
		sig.copy_from_slice(&bytes);
		Ok(BlsSignature(sig))
	}
}

// Conversion traits for easier usage
impl From<[u8; 48]> for BlsPublicKey {
	/// Create a new BlsPublicKey containing the provided raw key bytes.
	///
	/// # Examples
	///
	/// ```ignore
	/// let raw = [0u8; 48];
	/// let pk = BlsPublicKey::from(raw);
	/// assert_eq!(pk.0, raw);
	/// ```ignore
	fn from(bytes: [u8; 48]) -> Self {
		BlsPublicKey(bytes)
	}
}

impl From<BlsPublicKey> for [u8; 48] {
	/// Extracts the inner 48-byte array from a `BlsPublicKey`.
	///
	/// # Examples
	///
	/// ```ignore
	/// let arr = [0u8; 48];
	/// let pk = BlsPublicKey(arr);
	/// let bytes: [u8; 48] = pk.into();
	/// assert_eq!(bytes, arr);
	/// ```ignore
	fn from(key: BlsPublicKey) -> Self {
		key.0
	}
}

impl AsRef<[u8; 48]> for BlsPublicKey {
	/// Accesses the underlying 48-byte BLS public key array.
	///
	/// # Returns
	///
	/// A reference to the inner `[u8; 48]` containing the public key bytes.
	///
	/// # Examples
	///
	/// ```ignore
	/// let key = BlsPublicKey([0u8; 48]);
	/// assert_eq!(key.as_ref(), &[0u8; 48]);
	/// ```ignore
	fn as_ref(&self) -> &[u8; 48] {
		&self.0
	}
}

impl From<[u8; 96]> for BlsSignature {
	/// Constructs a `BlsSignature` from a 96-byte array.
	///
	/// # Examples
	///
	/// ```ignore
	/// let bytes = [0u8; 96];
	/// let sig = BlsSignature::from(bytes);
	/// assert_eq!(sig.0, bytes);
	/// ```ignore
	fn from(bytes: [u8; 96]) -> Self {
		BlsSignature(bytes)
	}
}

impl From<BlsSignature> for [u8; 96] {
	/// Consumes a `BlsSignature` and returns its inner 96-byte array.
	///
	/// # Examples
	///
	/// ```ignore
	/// let sig = BlsSignature([0u8; 96]);
	/// let arr: [u8; 96] = sig.into();
	/// assert_eq!(arr, [0u8; 96]);
	/// ```ignore
	fn from(sig: BlsSignature) -> Self {
		sig.0
	}
}

impl AsRef<[u8; 96]> for BlsSignature {
	/// Get a reference to the signature's underlying 96-byte array.
	///
	/// # Examples
	///
	/// ```ignore
	/// let sig = BlsSignature([1u8; 96]);
	/// let bytes: &[u8; 96] = sig.as_ref();
	/// assert_eq!(bytes[0], 1);
	/// assert_eq!(bytes.len(), 96);
	/// ```ignore
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

	/// Application gateway domain separator for delegation and constraint signatures.
	///
	/// This returns the 4-byte domain value used when constructing domain-separated signatures
	/// for the application gateway. The function returns a default value that may be overridden
	/// by configuration.
	///
	/// # Examples
	///
	/// ```ignore
	/// let domain = crate::domains::application_gateway_domain();
	/// assert_eq!(domain, [0x00, 0x00, 0x00, 0x02]);
	/// ```ignore
	///
	/// # Returns
	///
	/// A 4-byte array containing the domain separator.
	pub fn application_gateway_domain() -> [u8; 4] {
		// Default value, should be overridden by configuration
		[0x00, 0x00, 0x00, 0x02]
	}
}

/// Helper functions for delegation processing
impl SignedDelegation {
	/// Determines whether the delegation's slot equals the provided slot.
	///
	/// # Examples
	///
	/// ```ignore
	/// use crate::types::delegation::{DelegationMessage, SignedDelegation, BlsPublicKey, BlsSignature};
	///
	/// let msg = DelegationMessage {
	///     proposer: BlsPublicKey([0u8; 48]),
	///     delegate: BlsPublicKey([0u8; 48]),
	///     committer: String::from("0x"),
	///     slot: 42,
	/// };
	/// let sd = SignedDelegation {
	///     message: msg,
	///     signature: BlsSignature([0u8; 96]),
	/// };
	///
	/// assert!(sd.is_valid_for_slot(42));
	/// assert!(!sd.is_valid_for_slot(1));
	/// ```ignore
	pub fn is_valid_for_slot(&self, slot: u64) -> bool {
		self.message.slot == slot
	}

	/// Get the committer ECDSA address associated with the delegation message.
	///
	/// The address is stored as a hex string (typically an ECDSA address).
	///
	/// # Examples
	///
	/// ```ignore
	/// let sd = SignedDelegation {
	///     message: DelegationMessage {
	///         proposer: BlsPublicKey([0u8; 48]),
	///         delegate: BlsPublicKey([0u8; 48]),
	///         committer: "0xabc123".to_string(),
	///         slot: 0,
	///     },
	///     signature: BlsSignature([0u8; 96]),
	/// };
	/// assert_eq!(sd.get_committer_address(), "0xabc123");
	/// ```ignore
	pub fn get_committer_address(&self) -> &str {
		&self.message.committer
	}

	/// Returns the delegate BLS public key used for constraint signing.
	///
	/// # Examples
	///
	/// ```ignore
	/// let sd = SignedDelegation {
	///     message: DelegationMessage {
	///         proposer: BlsPublicKey([0u8; 48]),
	///         delegate: BlsPublicKey([1u8; 48]),
	///         committer: String::from("0x0"),
	///         slot: 0,
	///     },
	///     signature: BlsSignature([0u8; 96]),
	/// };
	/// let key: &BlsPublicKey = sd.get_delegate_key();
	/// assert_eq!(key.as_ref(), &[1u8; 48]);
	/// ```ignore
	pub fn get_delegate_key(&self) -> &BlsPublicKey {
		&self.message.delegate
	}

	/// Accesses the proposer BLS public key from the signed delegation message.
	///
	/// # Examples
	///
	/// ```ignore
	/// // assuming `sd` is a SignedDelegation
	/// let proposer_key = sd.get_proposer_key();
	/// let proposer_bytes: &[u8; 48] = proposer_key.as_ref();
	/// ```ignore
	pub fn get_proposer_key(&self) -> &BlsPublicKey {
		&self.message.proposer
	}

	/// Accesses the delegate's BLS public key bytes.
	///
	/// # Returns
	///
	/// `&[u8; 48]` reference to the delegate public key bytes.
	///
	/// # Examples
	///
	/// ```ignore
	/// let bytes: &[u8; 48] = signed_delegation.get_delegate_bytes();
	/// assert_eq!(bytes.len(), 48);
	/// ```ignore
	pub fn get_delegate_bytes(&self) -> &[u8; 48] {
		&self.message.delegate.0
	}

	/// Access the proposer BLS public key bytes.
	///
	/// # Returns
	///
	/// A reference to the proposer's public key as a 48-byte array.
	///
	/// # Examples
	///
	/// ```ignore
	/// let sd: SignedDelegation = /* obtain SignedDelegation */ unimplemented!();
	/// let bytes: &[u8; 48] = sd.get_proposer_bytes();
	/// assert_eq!(bytes.len(), 48);
	/// ```ignore
	pub fn get_proposer_bytes(&self) -> &[u8; 48] {
		&self.message.proposer.0
	}

	/// Accesses the underlying BLS signature bytes.
	///
	/// # Examples
	///
	/// ```ignore
	/// let signed = SignedDelegation {
	///     message: DelegationMessage {
	///         proposer: BlsPublicKey([0u8; 48]),
	///         delegate: BlsPublicKey([0u8; 48]),
	///         committer: String::from("0x00"),
	///         slot: 0,
	///     },
	///     signature: BlsSignature([1u8; 96]),
	/// };
	/// let bytes = signed.get_signature_bytes();
	/// assert_eq!(bytes.len(), 96);
	/// assert_eq!(bytes[0], 1);
	/// ```ignore
	///
	/// # Returns
	///
	/// A reference to the 96-byte signature array.
	pub fn get_signature_bytes(&self) -> &[u8; 96] {
		&self.signature.0
	}
}

impl ConstraintsMessage {
	/// Constructs a `ConstraintsMessage` from the provided fields.
	///
	/// If `receivers` is empty, the message is considered public (no receiver restrictions).
	///
	/// # Examples
	///
	/// ```ignore
	/// let proposer = BlsPublicKey([0u8; 48]);
	/// let delegate = BlsPublicKey([1u8; 48]);
	/// let constraints = Vec::<Constraint>::new();
	/// let receivers = Vec::<BlsPublicKey>::new();
	/// let msg = ConstraintsMessage::new(proposer.clone(), delegate.clone(), 42, constraints, receivers);
	/// assert_eq!(msg.slot, 42);
	/// assert_eq!(msg.get_proposer_key().as_ref(), &proposer.0);
	/// assert_eq!(msg.get_delegate_key().as_ref(), &delegate.0);
	/// ```ignore
	pub fn new(
		proposer: BlsPublicKey,
		delegate: BlsPublicKey,
		slot: u64,
		constraints: Vec<Constraint>,
		receivers: Vec<BlsPublicKey>,
	) -> Self {
		Self { proposer, delegate, slot, constraints, receivers }
	}

	/// Appends a constraint to the message's constraint list, preserving insertion order.
	///
	/// # Examples
	///
	/// ```ignore
	/// let proposer = BlsPublicKey([0u8; 48]);
	/// let delegate = BlsPublicKey([1u8; 48]);
	/// let mut msg = ConstraintsMessage::new(proposer, delegate, 42, vec![], vec![]);
	/// msg.add_constraint(Constraint::from_inclusion_commitment(vec![1, 2, 3]));
	/// assert_eq!(msg.constraints.len(), 1);
	/// ```ignore
	pub fn add_constraint(&mut self, constraint: Constraint) {
		self.constraints.push(constraint);
	}
}

impl Constraint {
	/// Constructs an Inclusion Preconfirmation constraint from the given commitment payload.
	///
	/// The resulting `Constraint` has `constraint_type` set to `1` and stores `payload` verbatim.
	///
	/// # Examples
	///
	/// ```ignore
	/// let payload = vec![0xAA, 0xBB];
	/// let c = Constraint::from_inclusion_commitment(payload.clone());
	/// assert_eq!(c.constraint_type, 1);
	/// assert_eq!(c.payload, payload);
	/// ```ignore
	pub fn from_inclusion_commitment(payload: Vec<u8>) -> Self {
		Self {
			constraint_type: 1, // Inclusion Preconfirmation
			payload,
		}
	}
}
