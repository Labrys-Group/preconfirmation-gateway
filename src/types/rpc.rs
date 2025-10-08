use serde::{Deserialize, Serialize};

/// Custom serde module for hex string <-> Vec<u8> conversion
mod hex_bytes {
	use serde::{Deserialize, Deserializer, Serializer};

	pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let hex_string = format!("0x{}", hex::encode(bytes));
		serializer.serialize_str(&hex_string)
	}

	pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
	where
		D: Deserializer<'de>,
	{
		let s = String::deserialize(deserializer)?;
		let s = s.strip_prefix("0x").unwrap_or(&s);
		hex::decode(s).map_err(serde::de::Error::custom)
	}
}

/// Request for a new SignedCommitment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitmentRequest {
	pub commitment_type: u64,
	#[serde(with = "hex_bytes")]
	pub payload: Vec<u8>,
	pub slasher: String, // address as hex string
}

/// Core commitment data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commitment {
	pub commitment_type: u64,
	#[serde(with = "hex_bytes")]
	pub payload: Vec<u8>,
	pub request_hash: String, // bytes32 as hex string
	pub slasher: String,      // address as hex string
}

/// A commitment with its ECDSA signature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCommitment {
	pub commitment: Commitment,
	pub signature: String, // ECDSA signature as hex string
}

/// Information about offerings for a specific chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Offering {
	pub chain_id: u64,
	pub commitment_types: Vec<u64>,
}

/// Information about a specific slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotInfo {
	pub slot: u64,
	pub offerings: Vec<Offering>,
}

/// Fee information for a commitment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeInfo {
	#[serde(with = "hex_bytes")]
	pub fee_payload: Vec<u8>, // opaque fee payload
	pub commitment_type: u64,
}
