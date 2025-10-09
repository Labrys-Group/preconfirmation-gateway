/// Shared serde module for hex string <-> Vec<u8> conversion
///
/// This module provides serialization and deserialization functions for converting
/// between hex-encoded strings (with optional "0x" prefix) and byte vectors.
///
/// # Usage
use serde::{Deserialize, Deserializer, Serializer};

/// Serialize a byte vector as a hex string prefixed with "0x".
///
/// This serializer encodes the provided `Vec<u8>` to hex, prefixes it with `0x`,
/// and writes it as a JSON string.
///
/// # Examples
///
pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
	S: Serializer,
{
	let hex_string = format!("0x{}", hex::encode(bytes));
	serializer.serialize_str(&hex_string)
}

/// Deserialize a hex-encoded string (accepts an optional "0x" prefix) into a byte vector.
///
/// The deserializer expects a string value, removes a leading `0x` if present,
/// decodes the remaining hex characters into bytes, and returns the resulting `Vec<u8>`.
///
/// # Examples
///
pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
	D: Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	let s = s.strip_prefix("0x").unwrap_or(&s);
	hex::decode(s).map_err(serde::de::Error::custom)
}
