//! Unit tests for cryptographic functions
//!
//! These tests verify the ECDSA signing, verification, and hashing functions
//! work correctly according to the Gateway specification.

use preconfirmation_gateway::{
	crypto::{
		sign_commitment, verify_commitment_signature, generate_request_hash,
		abi_encode_commitment, abi_encode_commitment_request, keccak256
	},
	types::{Commitment, CommitmentRequest}
};
use secp256k1::{SecretKey, Secp256k1};
use hex;

/// Generate a test private key for consistent testing
fn test_private_key() -> SecretKey {
	let key_bytes = hex::decode("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
		.expect("Failed to decode test private key");
	SecretKey::from_slice(&key_bytes).expect("Invalid private key")
}

/// Create a test commitment for testing
fn test_commitment() -> Commitment {
	Commitment {
		commitment_type: 1,
		payload: vec![1, 2, 3, 4, 5],
		request_hash: "0x1234567890123456789012345678901234567890123456789012345678901234".to_string(),
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	}
}

/// Create a test commitment request for testing
fn test_commitment_request() -> CommitmentRequest {
	CommitmentRequest {
		commitment_type: 1,
		payload: vec![1, 2, 3, 4, 5],
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	}
}

#[test]
fn test_keccak256_hash() {
	let input = b"hello world";
	let hash = keccak256(input);

	// Expected keccak256 hash of "hello world"
	let expected = hex::decode("47173285a8d7341e5e972fc677286384f802f8ef42a5ec5f03bbfa254cb01fad")
		.expect("Failed to decode expected hash");

	assert_eq!(hash.to_vec(), expected);
}

#[test]
fn test_keccak256_empty_input() {
	let input = b"";
	let hash = keccak256(input);

	// Expected keccak256 hash of empty string
	let expected = hex::decode("c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470")
		.expect("Failed to decode expected hash");

	assert_eq!(hash.to_vec(), expected);
}

#[test]
fn test_abi_encode_commitment_request() {
	let request = test_commitment_request();
	let result = abi_encode_commitment_request(&request);

	assert!(result.is_ok());
	let encoded = result.unwrap();
	assert!(!encoded.is_empty());

	// Verify the encoding contains the expected components
	// The encoding should include commitment_type, payload, and slasher
	assert!(encoded.len() > 32); // Should be more than just one word
}

#[test]
fn test_abi_encode_commitment() {
	let commitment = test_commitment();
	let result = abi_encode_commitment(&commitment);

	assert!(result.is_ok());
	let encoded = result.unwrap();
	assert!(!encoded.is_empty());

	// Verify the encoding contains the expected components
	assert!(encoded.len() > 32); // Should be more than just one word
}

#[test]
fn test_generate_request_hash() {
	let request = test_commitment_request();
	let result = generate_request_hash(&request);

	assert!(result.is_ok());
	let hash = result.unwrap();

	// Verify hash format
	assert!(hash.starts_with("0x"));
	assert_eq!(hash.len(), 66); // 0x + 64 hex characters

	// Verify it's valid hex
	let decoded = hex::decode(&hash[2..]);
	assert!(decoded.is_ok());
	assert_eq!(decoded.unwrap().len(), 32); // 32 bytes
}

#[test]
fn test_generate_request_hash_deterministic() {
	let request = test_commitment_request();

	// Generate hash multiple times
	let hash1 = generate_request_hash(&request).unwrap();
	let hash2 = generate_request_hash(&request).unwrap();
	let hash3 = generate_request_hash(&request).unwrap();

	// Should be identical (deterministic)
	assert_eq!(hash1, hash2);
	assert_eq!(hash2, hash3);
}

#[test]
fn test_generate_request_hash_different_inputs() {
	let request1 = test_commitment_request();
	let mut request2 = test_commitment_request();
	request2.payload = vec![6, 7, 8, 9, 10]; // Different payload

	let hash1 = generate_request_hash(&request1).unwrap();
	let hash2 = generate_request_hash(&request2).unwrap();

	// Should be different
	assert_ne!(hash1, hash2);
}

#[test]
fn test_sign_commitment() {
	let commitment = test_commitment();
	let private_key = test_private_key();

	let result = sign_commitment(&commitment, &private_key);

	assert!(result.is_ok());
	let signature = result.unwrap();

	// Verify signature format
	assert!(signature.starts_with("0x"));
	assert_eq!(signature.len(), 130); // 0x + 128 hex characters (64 bytes)

	// Verify it's valid hex
	let decoded = hex::decode(&signature[2..]);
	assert!(decoded.is_ok());
	assert_eq!(decoded.unwrap().len(), 64); // 64 bytes
}

#[test]
fn test_sign_commitment_deterministic() {
	let commitment = test_commitment();
	let private_key = test_private_key();

	// Sign multiple times
	let sig1 = sign_commitment(&commitment, &private_key).unwrap();
	let sig2 = sign_commitment(&commitment, &private_key).unwrap();
	let sig3 = sign_commitment(&commitment, &private_key).unwrap();

	// Signatures should be identical (deterministic)
	assert_eq!(sig1, sig2);
	assert_eq!(sig2, sig3);
}

#[test]
fn test_sign_different_commitments() {
	let commitment1 = test_commitment();
	let mut commitment2 = test_commitment();
	commitment2.payload = vec![6, 7, 8, 9, 10]; // Different payload

	let private_key = test_private_key();

	let sig1 = sign_commitment(&commitment1, &private_key).unwrap();
	let sig2 = sign_commitment(&commitment2, &private_key).unwrap();

	// Signatures should be different
	assert_ne!(sig1, sig2);
}

#[test]
fn test_verify_commitment_signature() {
	let commitment = test_commitment();
	let private_key = test_private_key();
	let secp = Secp256k1::new();
	let public_key = private_key.public_key(&secp);

	// Sign the commitment
	let signature = sign_commitment(&commitment, &private_key).unwrap();

	// Verify the signature
	let result = verify_commitment_signature(&commitment, &signature, &public_key);

	assert!(result.is_ok());
	assert!(result.unwrap(), "Signature verification should succeed");
}

#[test]
fn test_verify_commitment_signature_invalid() {
	let commitment = test_commitment();
	let private_key = test_private_key();
	let secp = Secp256k1::new();
	let public_key = private_key.public_key(&secp);

	// Create an invalid signature
	let invalid_signature = "0x1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890";

	// Verify should fail
	let result = verify_commitment_signature(&commitment, invalid_signature, &public_key);

	// Should either fail to parse or return false
	match result {
		Ok(valid) => assert!(!valid, "Invalid signature should not verify"),
		Err(_) => (), // Expected - invalid signature format
	}
}

#[test]
fn test_verify_commitment_signature_wrong_commitment() {
	let commitment1 = test_commitment();
	let mut commitment2 = test_commitment();
	commitment2.payload = vec![6, 7, 8, 9, 10]; // Different payload

	let private_key = test_private_key();
	let secp = Secp256k1::new();
	let public_key = private_key.public_key(&secp);

	// Sign commitment1
	let signature = sign_commitment(&commitment1, &private_key).unwrap();

	// Try to verify signature against commitment2 (should fail)
	let result = verify_commitment_signature(&commitment2, &signature, &public_key);

	assert!(result.is_ok());
	assert!(!result.unwrap(), "Signature should not verify for different commitment");
}

#[test]
fn test_verify_commitment_signature_wrong_public_key() {
	let commitment = test_commitment();
	let private_key1 = test_private_key();

	// Create different private key
	let key_bytes = hex::decode("1234567890123456789012345678901234567890123456789012345678901234")
		.expect("Failed to decode test private key");
	let private_key2 = SecretKey::from_slice(&key_bytes).expect("Invalid private key");

	let secp = Secp256k1::new();
	let public_key2 = private_key2.public_key(&secp);

	// Sign with private_key1
	let signature = sign_commitment(&commitment, &private_key1).unwrap();

	// Try to verify with public_key2 (should fail)
	let result = verify_commitment_signature(&commitment, &signature, &public_key2);

	assert!(result.is_ok());
	assert!(!result.unwrap(), "Signature should not verify with wrong public key");
}

#[test]
fn test_complete_sign_verify_cycle() {
	let commitment = test_commitment();
	let private_key = test_private_key();
	let secp = Secp256k1::new();
	let public_key = private_key.public_key(&secp);

	// Complete cycle: sign then verify
	let signature = sign_commitment(&commitment, &private_key).unwrap();
	let is_valid = verify_commitment_signature(&commitment, &signature, &public_key).unwrap();

	assert!(is_valid, "Complete sign-verify cycle should succeed");
}

#[test]
fn test_signature_format_consistency() {
	let commitment = test_commitment();
	let private_key = test_private_key();

	let signature = sign_commitment(&commitment, &private_key).unwrap();

	// Verify signature matches expected format
	assert!(signature.starts_with("0x"));
	assert_eq!(signature.len(), 130);

	// Verify all characters after 0x are valid hex
	let hex_part = &signature[2..];
	for c in hex_part.chars() {
		assert!(c.is_ascii_hexdigit(), "Non-hex character found in signature: {}", c);
	}
}