//! Cryptographic utilities for ECDSA and BLS signing
//!
//! This module implements the Gateway specification for cryptographic operations:
//! - Keccak256 hashing
//! - ABI encoding of commitment structures
//! - ECDSA signing using secp256k1 for commitments
//! - BLS signing for constraint messages and delegation verification

pub mod bls;

use anyhow::{Context, Result};
use ethabi::{Token, encode};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey, ecdsa::Signature};
use tiny_keccak::{Hasher, Keccak};

use crate::types::{Commitment, CommitmentRequest};

// Re-export BLS functionality for convenience

/// Computes the Keccak-256 hash of the given bytes.
///
/// # Examples
///
/// ```
/// let h = keccak256(b"hello world");
/// assert_eq!(h.len(), 32);
/// ```
pub fn keccak256(input: &[u8]) -> [u8; 32] {
	let mut keccak = Keccak::v256();
	keccak.update(input);
	let mut output = [0u8; 32];
	keccak.finalize(&mut output);
	output
}

/// Produces a 0x-prefixed Keccak-256 hash that binds a `CommitmentRequest` to its original request.
///
/// The function ABI-encodes the provided `CommitmentRequest`, computes the Keccak-256 digest of the encoded bytes,
/// and returns the result as a hex string prefixed with `0x`.
///
/// # Returns
///
/// A 0x-prefixed hex string containing the 32-byte Keccak-256 hash of the ABI-encoded request.
///
/// # Examples
///
/// ```
/// // Given a prepared `CommitmentRequest` named `req`:
/// let hash = generate_request_hash(&req).unwrap();
/// assert!(hash.starts_with("0x"));
/// assert_eq!(hash.len(), 66); // "0x" + 64 hex chars
/// ```
pub fn generate_request_hash(request: &CommitmentRequest) -> Result<String> {
	// ABI encode the CommitmentRequest
	let encoded = abi_encode_commitment_request(request)?;

	// Compute Keccak256 hash
	let hash = keccak256(&encoded);

	// Return as hex string with 0x prefix
	Ok(format!("0x{}", hex::encode(hash)))
}

/// Encode a CommitmentRequest into Ethereum ABI bytes following the Gateway specification.
///
/// The encoded tuple contains, in order:
/// 1. `commitment_type` encoded as an unsigned integer.
/// 2. `payload` encoded as raw bytes.
/// 3. `slasher` encoded as a 20-byte Ethereum address parsed from the request's string.
///
/// # Errors
///
/// Returns an error if the `slasher` address cannot be parsed or if encoding fails.
///
/// # Examples
///
/// ```
/// // Construct a simple CommitmentRequest and encode it.
/// // The actual field types of `CommitmentRequest` must match those used in your crate.
/// let request = CommitmentRequest {
///     commitment_type: 1u8,
///     payload: vec![0x01, 0x02],
///     slasher: "0x0000000000000000000000000000000000000000".to_string(),
/// };
///
/// let encoded = abi_encode_commitment_request(&request).expect("encoding failed");
/// assert!(!encoded.is_empty());
/// ```
pub fn abi_encode_commitment_request(request: &CommitmentRequest) -> Result<Vec<u8>> {
	let tokens = vec![
		Token::Uint(request.commitment_type.into()),
		Token::Bytes(request.payload.clone()),
		Token::Address(parse_ethereum_address(&request.slasher)?),
	];

	Ok(encode(&tokens))
}

/// Encode a Commitment into Ethereum ABI bytes according to the Gateway spec.
///
/// The encoded tuple contains, in order:
/// - `commitment_type` as an unsigned integer,
/// - `payload` as `bytes`,
/// - `request_hash` as a 32-byte fixed-length byte array parsed from hex,
/// - `slasher` as an Ethereum address parsed from hex.
///
/// # Examples
///
/// ```
/// // Construct a Commitment with appropriate fields and encode it.
/// // Types and constructors depend on the surrounding crate.
/// let commitment = Commitment {
///     commitment_type: 1u64.into(),
///     payload: vec![1, 2, 3],
///     request_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".into(),
///     slasher: "0x0000000000000000000000000000000000000000".into(),
/// };
/// let encoded = abi_encode_commitment(&commitment).unwrap();
/// assert!(!encoded.is_empty());
/// ```
pub fn abi_encode_commitment(commitment: &Commitment) -> Result<Vec<u8>> {
	let tokens = vec![
		Token::Uint(commitment.commitment_type.into()),
		Token::Bytes(commitment.payload.clone()),
		Token::FixedBytes(parse_hex_bytes(&commitment.request_hash, 32)?),
		Token::Address(parse_ethereum_address(&commitment.slasher)?),
	];

	Ok(encode(&tokens))
}

/// Signs a Commitment using ECDSA (secp256k1) according to the Gateway specification.
///
/// The signed message is the Keccak-256 hash of the ABI-encoded commitment. The returned
/// signature is the 64-byte compact form (r || s) encoded as a 0x-prefixed hex string.
///
/// # Returns
///
/// Ok with the signature as a `0x`-prefixed hex string containing 64 bytes (r||s) when signing succeeds,
/// or an `Err` if ABI encoding, hashing, message construction, or signing fails.
///
/// # Examples
///
/// ```
/// // Given `commitment: Commitment` and `sk: SecretKey` in scope:
/// let sig = sign_commitment(&commitment, &sk).unwrap();
/// assert!(sig.starts_with("0x"));
/// assert_eq!(hex::decode(&sig[2..]).unwrap().len(), 64);
/// ```
pub fn sign_commitment(commitment: &Commitment, private_key: &SecretKey) -> Result<String> {
	// 1. ABI encode the commitment
	let encoded = abi_encode_commitment(commitment).context("Failed to ABI encode commitment")?;

	// 2. Compute Keccak256 hash
	let message_hash = keccak256(&encoded);

	// 3. Create secp256k1 message
	let message = Message::from_slice(&message_hash).context("Failed to create message from hash")?;

	// 4. Sign with ECDSA - simple!
	let secp = Secp256k1::new();
	let signature = secp.sign_ecdsa(&message, private_key);

	// 5. Serialize as compact 64-byte (r || s) and return as hex
	let signature_bytes = signature.serialize_compact(); // 64 bytes (r + s)

	// Return as hex string with 0x prefix
	Ok(format!("0x{}", hex::encode(signature_bytes)))
}

/// Verifies an ECDSA signature for a commitment using the provided public key.
///
/// Returns `true` if the signature is valid for the commitment and public key, `false` otherwise.
///
/// # Examples
///
/// ```no_run
/// // `commitment`, `signature_hex`, and `public_key` are assumed to be constructed earlier.
/// let is_valid = crate::crypto::verify_commitment_signature(&commitment, &signature_hex, &public_key)
///     .expect("signature verification failed");
/// println!("signature valid: {}", is_valid);
/// ```
pub fn verify_commitment_signature(
	commitment: &Commitment,
	signature_hex: &str,
	public_key: &PublicKey,
) -> Result<bool> {
	// 1. ABI encode the commitment
	let encoded = abi_encode_commitment(commitment).context("Failed to ABI encode commitment")?;

	// 2. Compute Keccak256 hash
	let message_hash = keccak256(&encoded);

	// 3. Create secp256k1 message
	let message = Message::from_slice(&message_hash).context("Failed to create message from hash")?;

	// 4. Parse signature (64 bytes: r + s)
	let signature_bytes = parse_hex_bytes(signature_hex, 64)?;
	let signature = Signature::from_compact(&signature_bytes).context("Failed to parse signature")?;

	// 5. Verify signature
	let secp = Secp256k1::new();
	Ok(secp.verify_ecdsa(&message, &signature, public_key).is_ok())
}

/// Parses a 32-byte ECDSA (secp256k1) private key from a hex string.
///
/// Accepts hex strings with or without a `0x` prefix and returns a `SecretKey` on success.
/// Returns an error if hex decoding fails or the decoded bytes do not form a valid 32-byte private key.
///
/// # Examples
///
/// ```
/// let sk = parse_private_key("0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef").unwrap();
/// let _ = sk; // use the SecretKey as needed
/// ```
pub fn parse_private_key(hex_str: &str) -> Result<SecretKey> {
	let key_bytes = parse_hex_bytes(hex_str, 32)?;
	SecretKey::from_slice(&key_bytes).context("Invalid private key")
}

/// Parse an Ethereum address from a hex string into an `ethabi::Address`.
///
/// The input may include a `0x` prefix. Returns an error if the string is not valid hex
/// or does not represent exactly 20 bytes (the length of an Ethereum address).
///
/// # Examples
///
/// ```
/// let s = "0x11223344556677889900aabbccddeeff00112233";
/// let addr = crate::crypto::parse_ethereum_address(s).unwrap();
/// assert_eq!(addr.len(), 20);
/// ```
fn parse_ethereum_address(address_str: &str) -> Result<ethabi::Address> {
	let address_bytes = parse_hex_bytes(address_str, 20)?;
	Ok(ethabi::Address::from_slice(&address_bytes))
}

/// Converts a hex string (with optional `0x` prefix) into a byte vector of a specified length.

///

/// Returns an error if the string is not valid hex or if the decoded byte length does not equal `expected_len`.

///

/// # Parameters

///

/// - `hex_str`: Hex-encoded input string, may start with `0x`.

/// - `expected_len`: Required length of the resulting byte vector in bytes.

///

/// # Returns

///

/// A `Vec<u8>` containing the decoded bytes of length `expected_len`.

///

/// # Examples

///

/// ```

/// let b = crate::crypto::parse_hex_bytes("0x0102ff", 3).unwrap();

/// assert_eq!(b, vec![0x01, 0x02, 0xff]);

/// ```
pub fn parse_hex_bytes(hex_str: &str, expected_len: usize) -> Result<Vec<u8>> {
	let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
	let bytes = hex::decode(hex_str).context("Invalid hex string")?;

	if bytes.len() != expected_len {
		anyhow::bail!("Expected {} bytes, got {}", expected_len, bytes.len());
	}

	Ok(bytes)
}

/// Derives the Ethereum address corresponding to an ECDSA secp256k1 private key.
///
/// Returns the canonical 0x-prefixed hexadecimal Ethereum address produced from the
/// uncompressed public key (last 20 bytes of the Keccak-256 hash).
///
/// # Examples
///
/// ```
/// use secp256k1::SecretKey;
/// // Example private key: 32 bytes with value 1
/// let sk = SecretKey::from_slice(&[1u8; 32]).unwrap();
/// let addr = ecdsa_to_address(&sk).unwrap();
/// assert!(addr.starts_with("0x") && addr.len() == 42);
/// ```
pub fn ecdsa_to_address(private_key: &SecretKey) -> Result<String> {
	let secp = Secp256k1::new();
	let public_key = PublicKey::from_secret_key(&secp, private_key);

	// Get uncompressed public key (65 bytes: 0x04 + 32 + 32)
	let public_key_bytes = public_key.serialize_uncompressed();

	// Skip the 0x04 prefix and hash the remaining 64 bytes
	let hash = keccak256(&public_key_bytes[1..]);

	// Take last 20 bytes and format as hex address
	let address_bytes = &hash[12..];
	Ok(format!("0x{}", hex::encode(address_bytes)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::{Commitment, CommitmentRequest};

	#[test]
	fn test_keccak256() {
		// Test against known Keccak256 hash
		let input = b"hello world";
		let hash = keccak256(input);
		let expected = "47173285a8d7341e5e972fc677286384f802f8ef42a5ec5f03bbfa254cb01fad";
		assert_eq!(hex::encode(hash), expected);
	}

	#[test]
	fn test_generate_request_hash() {
		let request = CommitmentRequest {
			commitment_type: 1,
			payload: vec![1, 2, 3, 4],
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		let hash = generate_request_hash(&request).unwrap();
		assert!(hash.starts_with("0x"));
		assert_eq!(hash.len(), 66); // 0x + 64 hex chars
	}

	#[test]
	fn test_sign_and_verify_commitment() {
		// Generate test key pair
		let secp = Secp256k1::new();
		let (secret_key, public_key) = secp.generate_keypair(&mut secp256k1::rand::thread_rng());

		let commitment = Commitment {
			commitment_type: 1,
			payload: vec![1, 2, 3, 4],
			request_hash: "0x1234567890123456789012345678901234567890123456789012345678901234".to_string(),
			slasher: "0x1234567890123456789012345678901234567890".to_string(),
		};

		// Sign commitment
		let signature = sign_commitment(&commitment, &secret_key).unwrap();
		assert!(signature.starts_with("0x"));
		assert_eq!(signature.len(), 130); // 0x + 128 hex chars (64 bytes)

		// Verify signature
		let is_valid = verify_commitment_signature(&commitment, &signature, &public_key).unwrap();
		assert!(is_valid);
	}

	#[test]
	fn test_parse_private_key() {
		let key_hex = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
		let secret_key = parse_private_key(key_hex).unwrap();

		// Should be able to compute public key
		let secp = Secp256k1::new();
		let _public_key = PublicKey::from_secret_key(&secp, &secret_key);
	}
}
