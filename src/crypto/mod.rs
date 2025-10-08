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

/// Compute Keccak256 hash of input bytes
pub fn keccak256(input: &[u8]) -> [u8; 32] {
	let mut keccak = Keccak::v256();
	keccak.update(input);
	let mut output = [0u8; 32];
	keccak.finalize(&mut output);
	output
}

/// Generate request hash from a CommitmentRequest
///
/// According to the spec, this binds a commitment to its original request
pub fn generate_request_hash(request: &CommitmentRequest) -> Result<String> {
	// ABI encode the CommitmentRequest
	let encoded = abi_encode_commitment_request(request)?;

	// Compute Keccak256 hash
	let hash = keccak256(&encoded);

	// Return as hex string with 0x prefix
	Ok(format!("0x{}", hex::encode(hash)))
}

/// ABI encode a CommitmentRequest according to the Gateway spec
pub fn abi_encode_commitment_request(request: &CommitmentRequest) -> Result<Vec<u8>> {
	let tokens = vec![
		Token::Uint(request.commitment_type.into()),
		Token::Bytes(request.payload.clone()),
		Token::Address(parse_ethereum_address(&request.slasher)?),
	];

	Ok(encode(&tokens))
}

/// ABI encode a Commitment according to the Gateway spec
///
/// The Gateway spec states: message = keccak256(abi.encode(commitment))
pub fn abi_encode_commitment(commitment: &Commitment) -> Result<Vec<u8>> {
	let tokens = vec![
		Token::Uint(commitment.commitment_type.into()),
		Token::Bytes(commitment.payload.clone()),
		Token::FixedBytes(parse_hex_bytes(&commitment.request_hash, 32)?),
		Token::Address(parse_ethereum_address(&commitment.slasher)?),
	];

	Ok(encode(&tokens))
}

/// Sign a commitment using ECDSA according to the Gateway spec
///
/// Spec: message = keccak256(abi.encode(commitment))
///       signature = ECDSA.sign(message, committer_private_key)
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

	// 5. Serialize as DER-encoded bytes and return as hex
	let signature_bytes = signature.serialize_compact(); // 64 bytes (r + s)

	// Return as hex string with 0x prefix
	Ok(format!("0x{}", hex::encode(signature_bytes)))
}

/// Verify an ECDSA signature for a commitment
///
/// This is useful for testing and validation
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

/// Parse a private key from hex string
pub fn parse_private_key(hex_str: &str) -> Result<SecretKey> {
	let key_bytes = parse_hex_bytes(hex_str, 32)?;
	SecretKey::from_slice(&key_bytes).context("Invalid private key")
}

/// Parse an Ethereum address from hex string to ethabi::Address
fn parse_ethereum_address(address_str: &str) -> Result<ethabi::Address> {
	let address_bytes = parse_hex_bytes(address_str, 20)?;
	Ok(ethabi::Address::from_slice(&address_bytes))
}

/// Parse hex string (with or without 0x prefix) to bytes
pub fn parse_hex_bytes(hex_str: &str, expected_len: usize) -> Result<Vec<u8>> {
	let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
	let bytes = hex::decode(hex_str).context("Invalid hex string")?;

	if bytes.len() != expected_len {
		anyhow::bail!("Expected {} bytes, got {}", expected_len, bytes.len());
	}

	Ok(bytes)
}

/// Derive Ethereum address from ECDSA private key
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
