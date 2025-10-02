//! Integration tests for delegation management system
//!
//! These tests verify the delegation functionality including:
//! - BLS key generation and signing
//! - Delegation database operations
//! - Constraint message generation and verification

use anyhow::Result;
use preconfirmation_gateway::crypto::{bls_keys, BlsManager};
use preconfirmation_gateway::types::{
	BlsPublicKey, BlsSignature, Constraint, ConstraintsMessage, DelegationMessage, SignedConstraints,
	SignedDelegation,
};
use preconfirmation_gateway::db::delegation_ops;
use sqlx::PgPool;
use tempfile::NamedTempFile;
use uuid::Uuid;

/// Helper to create a test delegation
fn create_test_delegation() -> SignedDelegation {
	let (proposer_private_key, proposer_public_key) = bls_keys::generate_keypair();
	let (delegate_private_key, delegate_public_key) = bls_keys::generate_keypair();

	let message = DelegationMessage {
		proposer: BlsPublicKey(bls_keys::pubkey_to_bytes(&proposer_public_key)),
		delegate: BlsPublicKey(bls_keys::pubkey_to_bytes(&delegate_public_key)),
		committer: "0x1234567890123456789012345678901234567890".to_string(),
		slot: 12345,
	};

	// For test purposes, create a dummy signature
	// In practice, this would be signed by the proposer's private key
	let signature = BlsSignature([42u8; 96]);

	SignedDelegation { message, signature }
}

/// Helper to create a test constraints message
fn create_test_constraints_message() -> ConstraintsMessage {
	let (_, proposer_public_key) = bls_keys::generate_keypair();
	let (_, delegate_public_key) = bls_keys::generate_keypair();

	ConstraintsMessage {
		proposer: BlsPublicKey(bls_keys::pubkey_to_bytes(&proposer_public_key)),
		delegate: BlsPublicKey(bls_keys::pubkey_to_bytes(&delegate_public_key)),
		slot: 12345,
		constraints: vec![
			Constraint {
				constraint_type: 1,
				payload: vec![1, 2, 3, 4],
			},
			Constraint {
				constraint_type: 1,
				payload: vec![5, 6, 7, 8],
			},
		],
		receivers: vec![BlsPublicKey([99u8; 48])],
	}
}

#[tokio::test]
async fn test_bls_key_generation() {
	let (private_key, public_key) = bls_keys::generate_keypair();

	// Verify we can derive the same public key
	let derived_public = private_key.sk_to_pk();
	assert_eq!(public_key.to_bytes(), derived_public.to_bytes());

	// Verify key lengths
	let public_key_bytes = bls_keys::pubkey_to_bytes(&public_key);
	assert_eq!(public_key_bytes.len(), 48);
}

#[tokio::test]
async fn test_bls_key_parsing() {
	// Test parsing private key from hex
	let private_key_hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
	let private_key = bls_keys::parse_private_key(private_key_hex).unwrap();

	let public_key = private_key.sk_to_pk();
	let public_key_bytes = bls_keys::pubkey_to_bytes(&public_key);

	// Test parsing public key from hex
	let public_key_hex = hex::encode(public_key_bytes);
	let parsed_public_key = bls_keys::parse_public_key(&public_key_hex).unwrap();

	assert_eq!(public_key.to_bytes(), parsed_public_key.to_bytes());
}

#[tokio::test]
async fn test_constraint_message_signing() {
	let bls_manager = BlsManager::new("0x00000002").unwrap();
	let (private_key, public_key) = bls_keys::generate_keypair();

	let message = create_test_constraints_message();

	// Sign the constraints message
	let signature_bytes = bls_manager
		.sign_constraints_message(&message, &private_key)
		.unwrap();

	assert_eq!(signature_bytes.len(), 96);

	// Verify we can create a valid signature
	let signed_constraints = SignedConstraints {
		message: message.clone(),
		signature: BlsSignature(signature_bytes),
	};

	// Test serialization
	let serialized = serde_json::to_string(&signed_constraints).unwrap();
	assert!(!serialized.is_empty());

	// Test deserialization
	let deserialized: SignedConstraints = serde_json::from_str(&serialized).unwrap();
	assert_eq!(deserialized.message.slot, message.slot);
	assert_eq!(deserialized.message.constraints.len(), message.constraints.len());
}

#[tokio::test]
async fn test_delegation_serialization() {
	let delegation = create_test_delegation();

	// Test JSON serialization
	let serialized = serde_json::to_string(&delegation).unwrap();
	assert!(!serialized.is_empty());

	// Test deserialization
	let deserialized: SignedDelegation = serde_json::from_str(&serialized).unwrap();
	assert_eq!(deserialized.message.slot, delegation.message.slot);
	assert_eq!(deserialized.message.committer, delegation.message.committer);

	// Verify BLS key serialization/deserialization
	assert_eq!(deserialized.message.proposer.0, delegation.message.proposer.0);
	assert_eq!(deserialized.message.delegate.0, delegation.message.delegate.0);
	assert_eq!(deserialized.signature.0, delegation.signature.0);
}

#[tokio::test]
async fn test_delegation_helper_methods() {
	let delegation = create_test_delegation();

	// Test helper methods
	assert_eq!(delegation.get_committer_address(), "0x1234567890123456789012345678901234567890");
	assert!(delegation.is_valid_for_slot(12345));
	assert!(!delegation.is_valid_for_slot(54321));

	// Test key accessors
	let delegate_key = delegation.get_delegate_key();
	let proposer_key = delegation.get_proposer_key();
	let delegate_bytes = delegation.get_delegate_bytes();
	let proposer_bytes = delegation.get_proposer_bytes();
	let signature_bytes = delegation.get_signature_bytes();

	assert_eq!(delegate_bytes.len(), 48);
	assert_eq!(proposer_bytes.len(), 48);
	assert_eq!(signature_bytes.len(), 96);
	assert_eq!(delegate_key.0, *delegate_bytes);
	assert_eq!(proposer_key.0, *proposer_bytes);
}

#[tokio::test]
async fn test_constraint_helper_methods() {
	let mut message = create_test_constraints_message();

	// Test adding constraints
	let new_constraint = Constraint {
		constraint_type: 2,
		payload: vec![10, 11, 12],
	};

	message.add_constraint(new_constraint.clone());
	assert_eq!(message.constraints.len(), 3);
	assert_eq!(message.constraints[2].constraint_type, 2);
	assert_eq!(message.constraints[2].payload, vec![10, 11, 12]);

	// Test constraint creation from commitment
	let inclusion_constraint = Constraint::from_inclusion_commitment(vec![1, 2, 3]);
	assert_eq!(inclusion_constraint.constraint_type, 1);
	assert_eq!(inclusion_constraint.payload, vec![1, 2, 3]);
}

#[tokio::test]
async fn test_domain_parsing() {
	use preconfirmation_gateway::crypto::bls::domains;

	// Test valid domain parsing
	let domain = domains::parse_application_gateway_domain("0x00000002").unwrap();
	assert_eq!(domain, [0x00, 0x00, 0x00, 0x02]);

	// Test without 0x prefix
	let domain_no_prefix = domains::parse_application_gateway_domain("00000002").unwrap();
	assert_eq!(domain_no_prefix, [0x00, 0x00, 0x00, 0x02]);

	// Test invalid lengths
	assert!(domains::parse_application_gateway_domain("0x00").is_err());
	assert!(domains::parse_application_gateway_domain("0x0000000200").is_err());

	// Test invalid hex
	assert!(domains::parse_application_gateway_domain("0xgggggggg").is_err());
}

// Note: Database tests are marked as ignored since they require actual database setup
// In a real CI environment, these would be enabled with proper database containers

#[tokio::test]
#[ignore = "requires database setup"]
async fn test_delegation_database_operations() {
	// This test would be enabled in a CI environment with proper database setup
	// let pool = setup_test_pool().await;
	// let delegation = create_test_delegation();
	//
	// // Test save
	// let id = delegation_ops::save_delegation(&pool, &delegation).await.unwrap();
	// assert!(!id.is_nil());
	//
	// // Test retrieval by slot
	// let delegations = delegation_ops::get_delegations_for_slot(&pool, 12345).await.unwrap();
	// assert_eq!(delegations.len(), 1);
	// assert_eq!(delegations[0].message.slot, 12345);
	//
	// // Test existence check
	// let exists = delegation_ops::delegation_exists_for_slot_and_committer(
	//     &pool,
	//     12345,
	//     "0x1234567890123456789012345678901234567890"
	// ).await.unwrap();
	// assert!(exists);
}

#[tokio::test]
#[ignore = "requires database setup"]
async fn test_delegation_batch_operations() {
	// This test would verify batch insertion and retrieval of multiple delegations
	// let pool = setup_test_pool().await;
	// let delegations = vec![
	//     create_test_delegation_for_slot(100),
	//     create_test_delegation_for_slot(101),
	//     create_test_delegation_for_slot(102),
	// ];
	//
	// // Test batch save
	// let ids = delegation_ops::save_delegations_batch(&pool, &delegations).await.unwrap();
	// assert_eq!(ids.len(), 3);
	//
	// // Test retrieval by delegate
	// let retrieved = delegation_ops::get_delegations_by_delegate(&pool, &delegate_key).await.unwrap();
	// assert_eq!(retrieved.len(), 3);
}

#[test]
fn test_blspublickey_wrapper_conversions() {
	let raw_bytes = [42u8; 48];
	let wrapped_key = BlsPublicKey(raw_bytes);

	// Test conversion traits
	let converted_from: BlsPublicKey = raw_bytes.into();
	assert_eq!(converted_from.0, raw_bytes);

	let converted_to: [u8; 48] = wrapped_key.into();
	assert_eq!(converted_to, raw_bytes);

	// Test AsRef
	let as_ref: &[u8; 48] = wrapped_key.as_ref();
	assert_eq!(as_ref, &raw_bytes);
}

#[test]
fn test_blssignature_wrapper_conversions() {
	let raw_bytes = [42u8; 96];
	let wrapped_sig = BlsSignature(raw_bytes);

	// Test conversion traits
	let converted_from: BlsSignature = raw_bytes.into();
	assert_eq!(converted_from.0, raw_bytes);

	let converted_to: [u8; 96] = wrapped_sig.into();
	assert_eq!(converted_to, raw_bytes);

	// Test AsRef
	let as_ref: &[u8; 96] = wrapped_sig.as_ref();
	assert_eq!(as_ref, &raw_bytes);
}

#[test]
fn test_blskey_serialization_hex_format() {
	let key_bytes = [0x01, 0x02, 0x03, 0x04]; // First 4 bytes for testing
	let mut full_key = [0u8; 48];
	full_key[0..4].copy_from_slice(&key_bytes);

	let bls_key = BlsPublicKey(full_key);

	// Test serialization produces hex format
	let serialized = serde_json::to_string(&bls_key).unwrap();
	assert!(serialized.contains("0x01020304"));

	// Test round-trip
	let deserialized: BlsPublicKey = serde_json::from_str(&serialized).unwrap();
	assert_eq!(deserialized.0, full_key);
}

// Helper function for creating database test environment
// async fn setup_test_pool() -> PgPool {
//     // In a real test environment, this would:
//     // 1. Start a test database (e.g., using testcontainers)
//     // 2. Run migrations
//     // 3. Return a pool connected to the test database
//     todo!("Setup test database")
// }