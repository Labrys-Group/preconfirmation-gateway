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
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey, ecdsa::RecoverableSignature, ecdsa::RecoveryId};
use tiny_keccak::{Hasher, Keccak};

use crate::types::{Commitment, CommitmentRequest};

// Re-export BLS functionality for convenience

/// Computes the Keccak-256 hash of the given bytes.
///
/// # Examples
///
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
/// signature is the 65-byte recoverable format (r || s || v) encoded as a 0x-prefixed hex string.
/// The recovery ID (v) is required for standard Ethereum signature verification.
///
/// # Returns
///
/// Ok with the signature as a `0x`-prefixed hex string containing 65 bytes (r||s||v) when signing succeeds,
/// or an `Err` if ABI encoding, hashing, message construction, or signing fails.
///
/// # Examples
///
pub fn sign_commitment(commitment: &Commitment, private_key: &SecretKey) -> Result<String> {
	// 1. ABI encode the commitment
	let encoded = abi_encode_commitment(commitment).context("Failed to ABI encode commitment")?;

	// 2. Compute Keccak256 hash
	let message_hash = keccak256(&encoded);

	// 3. Create secp256k1 message
	let message = Message::from_slice(&message_hash).context("Failed to create message from hash")?;

	// 4. Sign with recoverable ECDSA signature (includes recovery ID)
	let secp = Secp256k1::new();
	let recoverable_sig = secp.sign_ecdsa_recoverable(&message, private_key);

	// 5. Serialize as 65-byte recoverable format (r || s || v)
	let (recovery_id, signature_bytes) = recoverable_sig.serialize_compact();

	// Ethereum uses v = 27 or 28 (not 0 or 1)
	let v = recovery_id.to_i32() as u8 + 27;

	// Combine into 65-byte signature
	let mut full_signature = signature_bytes.to_vec();
	full_signature.push(v);

	// Return as hex string with 0x prefix
	Ok(format!("0x{}", hex::encode(full_signature)))
}

/// Verifies an ECDSA signature for a commitment using the provided public key.
///
/// Accepts 65-byte recoverable signatures (r || s || v) where v is the recovery ID.
/// Returns `true` if the signature is valid for the commitment and public key, `false` otherwise.
///
/// # Examples
///
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

	// 4. Parse 65-byte recoverable signature (r || s || v)
	let signature_bytes = parse_hex_bytes(signature_hex, 65)?;

	// Extract recovery ID (v parameter) - Ethereum uses 27/28, secp256k1 uses 0/1
	let v = signature_bytes[64];
	let recovery_id = if v >= 27 {
		RecoveryId::from_i32((v - 27) as i32).context("Invalid recovery ID")?
	} else {
		RecoveryId::from_i32(v as i32).context("Invalid recovery ID")?
	};

	// Parse recoverable signature
	let recoverable_sig = RecoverableSignature::from_compact(&signature_bytes[..64], recovery_id)
		.context("Failed to parse recoverable signature")?;

	// Convert to standard signature for verification
	let signature = recoverable_sig.to_standard();

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
		assert_eq!(signature.len(), 132); // 0x + 130 hex chars (65 bytes: r||s||v)

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
