//! BLS cryptographic operations for delegation and constraint signing
//!
//! This module implements BLS signature operations according to the Gateway specification:
//! - Domain separation for different message types
//! - Delegation signature verification
//! - Constraint message signing with domain separation
//! - ABI encoding for BLS signature operations

use anyhow::{Context, Result};
use blst::{
	min_pk::{PublicKey as BlsPublicKey, SecretKey as BlsSecretKey, Signature as BlsSignature},
	BLST_ERROR,
};
use ethabi::{encode, Token};

use crate::types::delegation::{BlsPublicKey as GatewayBlsPublicKey, BlsSignature as GatewayBlsSignature, ConstraintsMessage, DelegationMessage, SignedDelegation};
use super::keccak256;

/// Domain separation constants
pub mod domains {
	/// Domain separator for delegation signatures (from spec: 0x0044656c)
	pub const DELEGATION_DOMAIN_SEPARATOR: [u8; 4] = [0x00, 0x44, 0x65, 0x6c];

	/// Parse domain separator from hex configuration string
	pub fn parse_application_gateway_domain(hex_str: &str) -> Result<[u8; 4], anyhow::Error> {
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
		let bytes = hex::decode(hex_str)
			.map_err(|e| anyhow::anyhow!("Invalid domain hex: {}", e))?;

		if bytes.len() != 4 {
			anyhow::bail!("Domain separator must be 4 bytes, got {}", bytes.len());
		}

		let mut domain = [0u8; 4];
		domain.copy_from_slice(&bytes);
		Ok(domain)
	}
}

/// BLS signature operations for Gateway
pub struct BlsManager {
	/// Domain separator for constraint signatures
	application_gateway_domain: [u8; 4],
}

impl BlsManager {
	/// Create new BLS manager with configurable domain
	pub fn new(domain_hex: &str) -> Result<Self> {
		let application_gateway_domain = domains::parse_application_gateway_domain(domain_hex)
			.context("Invalid application gateway domain")?;

		Ok(Self { application_gateway_domain })
	}

	/// Verify a delegation signature against the proposer's public key
	pub fn verify_delegation_signature(&self, delegation: &SignedDelegation) -> Result<bool> {
		// 1. ABI encode the delegation message
		let encoded = self.abi_encode_delegation_message(&delegation.message)?;

		// 2. Calculate signing root with delegation domain
		let signing_root = self.calculate_signing_root(&encoded, &domains::DELEGATION_DOMAIN_SEPARATOR);

		// 3. Parse proposer's BLS public key
		let proposer_pubkey = BlsPublicKey::from_bytes(&delegation.message.proposer.0)
			.map_err(|e| anyhow::anyhow!("Invalid proposer public key: {:?}", e))?;

		// 4. Parse BLS signature
		let signature = BlsSignature::from_bytes(&delegation.signature.0)
			.map_err(|e| anyhow::anyhow!("Invalid BLS signature: {:?}", e))?;

		// 5. Verify signature
		let result = signature.verify(true, &signing_root, b"", &[], &proposer_pubkey, true);
		Ok(result == BLST_ERROR::BLST_SUCCESS)
	}

	/// Sign a constraints message using the Gateway's BLS private key
	pub fn sign_constraints_message(
		&self,
		message: &ConstraintsMessage,
		private_key: &BlsSecretKey,
	) -> Result<[u8; 96]> {
		// 1. ABI encode the constraints message
		let encoded = self.abi_encode_constraints_message(message)?;

		// 2. Calculate signing root with application gateway domain
		let signing_root = self.calculate_signing_root(&encoded, &self.application_gateway_domain);

		// 3. Sign the message
		let signature = private_key.sign(&signing_root, b"", &[]);

		// 4. Return signature bytes
		Ok(signature.to_bytes())
	}

	/// ABI encode a delegation message for signing
	fn abi_encode_delegation_message(&self, message: &DelegationMessage) -> Result<Vec<u8>> {
		let tokens = vec![
			Token::FixedBytes(message.proposer.0.to_vec()), // BLS public key (48 bytes)
			Token::FixedBytes(message.delegate.0.to_vec()), // BLS public key (48 bytes)
			Token::Address(self.parse_ethereum_address(&message.committer)?), // Ethereum address
			Token::Uint(message.slot.into()), // Slot number
		];

		Ok(encode(&tokens))
	}

	/// ABI encode a constraints message for signing
	fn abi_encode_constraints_message(&self, message: &ConstraintsMessage) -> Result<Vec<u8>> {
		// Encode individual constraints
		let constraint_tokens: Result<Vec<Token>, anyhow::Error> = message
			.constraints
			.iter()
			.map(|c| -> Result<Token, anyhow::Error> {
				Ok(Token::Tuple(vec![
					Token::Uint(c.constraint_type.into()),
					Token::Bytes(c.payload.clone()),
				]))
			})
			.collect();

		// Encode receivers list
		let receiver_tokens: Vec<Token> = message
			.receivers
			.iter()
			.map(|r| Token::FixedBytes(r.0.to_vec()))
			.collect();

		let tokens = vec![
			Token::FixedBytes(message.proposer.0.to_vec()), // BLS public key
			Token::FixedBytes(message.delegate.0.to_vec()), // BLS public key
			Token::Uint(message.slot.into()), // Slot number
			Token::Array(constraint_tokens?), // Constraints array
			Token::Array(receiver_tokens), // Receivers array
		];

		Ok(encode(&tokens))
	}

	/// Calculate signing root with domain separation
	fn calculate_signing_root(&self, message: &[u8], domain: &[u8; 4]) -> [u8; 32] {
		// Spec: signing_root = keccak256(abi.encodePacked(domain, message))
		let mut combined = Vec::new();
		combined.extend_from_slice(domain);
		combined.extend_from_slice(message);

		keccak256(&combined)
	}

	/// Parse Ethereum address string to ethabi::Address
	fn parse_ethereum_address(&self, address_str: &str) -> Result<ethabi::Address> {
		let hex_str = address_str.strip_prefix("0x").unwrap_or(address_str);
		let bytes = hex::decode(hex_str)
			.context("Invalid hex string")?;

		if bytes.len() != 20 {
			anyhow::bail!("Ethereum address must be 20 bytes, got {}", bytes.len());
		}

		Ok(ethabi::Address::from_slice(&bytes))
	}
}

/// Utility functions for BLS key management
pub mod keys {
	use super::*;
	use rand::Rng;

	/// Generate a new BLS key pair
	pub fn generate_keypair() -> (BlsSecretKey, BlsPublicKey) {
		// Use proper key generation with random seed
		use rand::Rng;
		let mut rng = rand::thread_rng();
		let mut seed = [0u8; 32];
		rng.fill(&mut seed);
		let private_key = BlsSecretKey::key_gen(&seed, b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_").unwrap();
		let public_key = private_key.sk_to_pk();
		(private_key, public_key)
	}

	/// Parse BLS private key from hex string
	pub fn parse_private_key(hex_str: &str) -> Result<BlsSecretKey> {
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
		let bytes = hex::decode(hex_str)
			.context("Invalid hex string")?;

		if bytes.len() != 32 {
			anyhow::bail!("BLS private key must be 32 bytes, got {}", bytes.len());
		}

		BlsSecretKey::from_bytes(&bytes)
			.map_err(|e| anyhow::anyhow!("Invalid BLS private key: {:?}", e))
	}

	/// Parse BLS public key from hex string
	pub fn parse_public_key(hex_str: &str) -> Result<BlsPublicKey> {
		let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
		let bytes = hex::decode(hex_str)
			.context("Invalid hex string")?;

		if bytes.len() != 48 {
			anyhow::bail!("BLS public key must be 48 bytes, got {}", bytes.len());
		}

		BlsPublicKey::from_bytes(&bytes)
			.map_err(|e| anyhow::anyhow!("Invalid BLS public key: {:?}", e))
	}

	/// Convert BLS public key to 48-byte array
	pub fn pubkey_to_bytes(pubkey: &BlsPublicKey) -> [u8; 48] {
		let bytes = pubkey.to_bytes();
		let mut result = [0u8; 48];
		result.copy_from_slice(&bytes);
		result
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::types::delegation::{Constraint, ConstraintsMessage, DelegationMessage};

	#[test]
	fn test_domain_parsing() {
		let domain = domains::parse_application_gateway_domain("0x00000002").unwrap();
		assert_eq!(domain, [0x00, 0x00, 0x00, 0x02]);

		let domain_no_prefix = domains::parse_application_gateway_domain("00000002").unwrap();
		assert_eq!(domain_no_prefix, [0x00, 0x00, 0x00, 0x02]);

		// Test invalid length
		assert!(domains::parse_application_gateway_domain("0x00").is_err());
		assert!(domains::parse_application_gateway_domain("0x0000000200").is_err());
	}

	#[test]
	fn test_key_generation() {
		let (private_key, public_key) = keys::generate_keypair();

		// Verify we can derive the same public key
		let derived_public = private_key.sk_to_pk();
		assert_eq!(public_key.to_bytes(), derived_public.to_bytes());
	}

	#[test]
	fn test_constraints_message_signing() {
		let bls_manager = BlsManager::new("0x00000002").unwrap();
		let (private_key, public_key) = keys::generate_keypair();

		let message = ConstraintsMessage {
			proposer: GatewayBlsPublicKey([1u8; 48]),
			delegate: GatewayBlsPublicKey(keys::pubkey_to_bytes(&public_key)),
			slot: 12345,
			constraints: vec![Constraint {
				constraint_type: 1,
				payload: vec![1, 2, 3, 4],
			}],
			receivers: vec![GatewayBlsPublicKey([2u8; 48])],
		};

		let signature_bytes = bls_manager
			.sign_constraints_message(&message, &private_key)
			.unwrap();

		assert_eq!(signature_bytes.len(), 96);

		// Verify we can parse the signature
		let signature = BlsSignature::from_bytes(&signature_bytes).unwrap();
		assert_eq!(signature.to_bytes(), signature_bytes);
	}

	#[test]
	fn test_delegation_message_encoding() {
		let bls_manager = BlsManager::new("0x00000002").unwrap();

		let message = DelegationMessage {
			proposer: GatewayBlsPublicKey([1u8; 48]),
			delegate: GatewayBlsPublicKey([2u8; 48]),
			committer: "0x1234567890123456789012345678901234567890".to_string(),
			slot: 12345,
		};

		let encoded = bls_manager.abi_encode_delegation_message(&message).unwrap();
		assert!(!encoded.is_empty());

		// Should be able to encode without errors
		// Actual validation would require known test vectors
	}
}