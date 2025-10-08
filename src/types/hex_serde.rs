/// Shared serde module for hex string <-> Vec<u8> conversion
///
/// This module provides serialization and deserialization functions for converting
/// between hex-encoded strings (with optional "0x" prefix) and byte vectors.
///
/// # Usage
/// ```
/// use serde::{Deserialize, Serialize};
/// use preconfirmation_gateway::types::hex_serde as hex_bytes;
///
/// #[derive(Serialize, Deserialize)]
/// struct Example {
///     #[serde(with = "hex_bytes")]
///     data: Vec<u8>,
/// }
/// ```
use serde::{Deserialize, Deserializer, Serializer};

/// Serialize a Vec<u8> as a hex string with "0x" prefix
pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
	S: Serializer,
{
	let hex_string = format!("0x{}", hex::encode(bytes));
	serializer.serialize_str(&hex_string)
}

/// Deserialize a hex string (with optional "0x" prefix) into a Vec<u8>
pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
	D: Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	let s = s.strip_prefix("0x").unwrap_or(&s);
	hex::decode(s).map_err(serde::de::Error::custom)
}
