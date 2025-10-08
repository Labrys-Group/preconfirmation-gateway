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

/// Serialize a byte vector as a hex string prefixed with "0x".
///
/// This serializer encodes the provided `Vec<u8>` to hex, prefixes it with `0x`,
/// and writes it as a JSON string.
///
/// # Examples
///
/// ```
/// use serde::Serialize;
///
/// mod types {
///     pub mod hex_serde {
///         // Re-export the functions from the actual module path in your crate.
///         pub use crate::types::hex_serde::{serialize, deserialize};
///     }
/// }
///
/// #[derive(Serialize)]
/// struct S {
///     #[serde(with = "crate::types::hex_serde")]
///     data: Vec<u8>,
/// }
///
/// let s = S { data: vec![1, 2, 3] };
/// let json = serde_json::to_string(&s).unwrap();
/// assert_eq!(json, r#"{"data":"0x010203"}"#);
/// ```
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
/// ```
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Debug, PartialEq)]
/// struct S {
///     #[serde(with = "crate::types::hex_serde")]
///     data: Vec<u8>,
/// }
///
/// let s1: S = serde_json::from_str(r#"{"data":"0x616263"}"#).unwrap();
/// assert_eq!(s1.data, b"abc".to_vec());
///
/// let s2: S = serde_json::from_str(r#"{"data":"616263"}"#).unwrap();
/// assert_eq!(s2.data, b"abc".to_vec());
/// ```
pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
	D: Deserializer<'de>,
{
	let s = String::deserialize(deserializer)?;
	let s = s.strip_prefix("0x").unwrap_or(&s);
	hex::decode(s).map_err(serde::de::Error::custom)
}