use serde_json::{json, from_str, to_string};
use preconfirmation_gateway::types::{
	CommitmentRequest, Commitment, SignedCommitment, Offering, SlotInfo, FeeInfo,
	DatabaseContext, RpcContext, SlotInfoResponse
};
use anyhow::Result;

// Test serialization and deserialization of core types

#[test]
fn test_commitment_request_serialization() {
	let request = CommitmentRequest {
		commitment_type: 42,
		payload: vec![0xde, 0xad, 0xbe, 0xef],
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	// Test serialization
	let serialized = to_string(&request).expect("Failed to serialize CommitmentRequest");
	assert!(serialized.contains("42"));
	assert!(serialized.contains("1234567890123456789012345678901234567890"));
	
	// Test deserialization
	let deserialized: CommitmentRequest = from_str(&serialized).expect("Failed to deserialize CommitmentRequest");
	assert_eq!(deserialized.commitment_type, request.commitment_type);
	assert_eq!(deserialized.payload, request.payload);
	assert_eq!(deserialized.slasher, request.slasher);
}

#[test]
fn test_commitment_request_from_json() {
	let json_str = r#"{"commitment_type":1,"payload":[1,2,3,4],"slasher":"0xabcd"}"#;
	let request: CommitmentRequest = from_str(json_str).expect("Failed to parse JSON");
	
	assert_eq!(request.commitment_type, 1);
	assert_eq!(request.payload, vec![1, 2, 3, 4]);
	assert_eq!(request.slasher, "0xabcd");
}

#[test]
fn test_commitment_request_invalid_json() {
	// Missing required fields
	let invalid_json = r#"{"commitment_type":1}"#;
	let result: Result<CommitmentRequest, _> = from_str(invalid_json);
	assert!(result.is_err());
	
	// Wrong field types
	let invalid_json2 = r#"{"commitment_type":"not_a_number","payload":[1,2,3],"slasher":"0xabcd"}"#;
	let result2: Result<CommitmentRequest, _> = from_str(invalid_json2);
	assert!(result2.is_err());
}

#[test]
fn test_commitment_serialization() {
	let commitment = Commitment {
		commitment_type: 123,
		payload: vec![0xFF, 0x00, 0xAB],
		request_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12".to_string(),
		slasher: "0x9876543210fedcba9876543210fedcba98765432".to_string(),
	};
	
	// Test round-trip serialization
	let serialized = to_string(&commitment).expect("Failed to serialize Commitment");
	let deserialized: Commitment = from_str(&serialized).expect("Failed to deserialize Commitment");
	
	assert_eq!(deserialized.commitment_type, commitment.commitment_type);
	assert_eq!(deserialized.payload, commitment.payload);
	assert_eq!(deserialized.request_hash, commitment.request_hash);
	assert_eq!(deserialized.slasher, commitment.slasher);
}

#[test]
fn test_signed_commitment_serialization() {
	let commitment = Commitment {
		commitment_type: 1,
		payload: vec![],
		request_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
		slasher: "0x0000000000000000000000000000000000000000".to_string(),
	};
	
	let signed_commitment = SignedCommitment {
		commitment,
		signature: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12".to_string(),
	};
	
	// Test serialization
	let serialized = to_string(&signed_commitment).expect("Failed to serialize SignedCommitment");
	let deserialized: SignedCommitment = from_str(&serialized).expect("Failed to deserialize SignedCommitment");
	
	assert_eq!(deserialized.commitment.commitment_type, signed_commitment.commitment.commitment_type);
	assert_eq!(deserialized.signature, signed_commitment.signature);
}

#[test]
fn test_offering_serialization() {
	let offering = Offering {
		chain_id: 1,
		commitment_types: vec![1, 2, 3, 100],
	};
	
	let serialized = to_string(&offering).expect("Failed to serialize Offering");
	let deserialized: Offering = from_str(&serialized).expect("Failed to deserialize Offering");
	
	assert_eq!(deserialized.chain_id, offering.chain_id);
	assert_eq!(deserialized.commitment_types, offering.commitment_types);
}

#[test]
fn test_slot_info_serialization() {
	let offerings = vec![
		Offering { chain_id: 1, commitment_types: vec![1, 2] },
		Offering { chain_id: 137, commitment_types: vec![3, 4, 5] },
	];
	
	let slot_info = SlotInfo {
		slot: 123456789,
		offerings,
	};
	
	let serialized = to_string(&slot_info).expect("Failed to serialize SlotInfo");
	let deserialized: SlotInfo = from_str(&serialized).expect("Failed to deserialize SlotInfo");
	
	assert_eq!(deserialized.slot, slot_info.slot);
	assert_eq!(deserialized.offerings.len(), 2);
	assert_eq!(deserialized.offerings[0].chain_id, 1);
	assert_eq!(deserialized.offerings[1].chain_id, 137);
}

#[test]
fn test_slot_info_response_serialization() {
	let slot_infos = vec![
		SlotInfo {
			slot: 100,
			offerings: vec![Offering { chain_id: 1, commitment_types: vec![1] }],
		},
		SlotInfo {
			slot: 101,
			offerings: vec![],
		},
	];
	
	let response = SlotInfoResponse { slots: slot_infos };
	
	let serialized = to_string(&response).expect("Failed to serialize SlotInfoResponse");
	let deserialized: SlotInfoResponse = from_str(&serialized).expect("Failed to deserialize SlotInfoResponse");
	
	assert_eq!(deserialized.slots.len(), 2);
	assert_eq!(deserialized.slots[0].slot, 100);
	assert_eq!(deserialized.slots[1].slot, 101);
	assert!(deserialized.slots[1].offerings.is_empty());
}

#[test]
fn test_fee_info_serialization() {
	let fee_info = FeeInfo {
		fee_payload: vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF],
		commitment_type: 42,
	};
	
	let serialized = to_string(&fee_info).expect("Failed to serialize FeeInfo");
	let deserialized: FeeInfo = from_str(&serialized).expect("Failed to deserialize FeeInfo");
	
	assert_eq!(deserialized.fee_payload, fee_info.fee_payload);
	assert_eq!(deserialized.commitment_type, fee_info.commitment_type);
}

// Test edge cases and validation

#[test]
fn test_empty_payload_handling() {
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![],
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	let serialized = to_string(&request).expect("Failed to serialize empty payload");
	let deserialized: CommitmentRequest = from_str(&serialized).expect("Failed to deserialize empty payload");
	
	assert!(deserialized.payload.is_empty());
}

#[test]
fn test_large_payload_handling() {
	let large_payload = vec![0xFF; 1000000]; // 1MB payload
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: large_payload.clone(),
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	let serialized = to_string(&request).expect("Failed to serialize large payload");
	let deserialized: CommitmentRequest = from_str(&serialized).expect("Failed to deserialize large payload");
	
	assert_eq!(deserialized.payload.len(), 1000000);
	assert_eq!(deserialized.payload, large_payload);
}

#[test]
fn test_zero_values() {
	let request = CommitmentRequest {
		commitment_type: 0,
		payload: vec![0],
		slasher: "0x0000000000000000000000000000000000000000".to_string(),
	};
	
	let serialized = to_string(&request).expect("Failed to serialize zero values");
	let deserialized: CommitmentRequest = from_str(&serialized).expect("Failed to deserialize zero values");
	
	assert_eq!(deserialized.commitment_type, 0);
	assert_eq!(deserialized.payload, vec![0]);
}

#[test]
fn test_max_values() {
	let request = CommitmentRequest {
		commitment_type: u64::MAX,
		payload: vec![0xFF; 256],
		slasher: "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF".to_string(),
	};
	
	let serialized = to_string(&request).expect("Failed to serialize max values");
	let deserialized: CommitmentRequest = from_str(&serialized).expect("Failed to deserialize max values");
	
	assert_eq!(deserialized.commitment_type, u64::MAX);
	assert_eq!(deserialized.payload.len(), 256);
	assert!(deserialized.payload.iter().all(|&b| b == 0xFF));
}

#[test]
fn test_unicode_handling() {
	// Test that slasher field can handle various string formats
	let test_slashers = vec![
		"0x1234567890123456789012345678901234567890".to_string(),
		"1234567890123456789012345678901234567890".to_string(), // Without 0x prefix
		"".to_string(), // Empty string
		"not_a_hex_string".to_string(), // Invalid format
		"0xABCDEF".to_string(), // Short format
	];
	
	for slasher in test_slashers {
		let request = CommitmentRequest {
			commitment_type: 1,
			payload: vec![1, 2, 3],
			slasher: slasher.clone(),
		};
		
		let serialized = to_string(&request).expect("Failed to serialize");
		let deserialized: CommitmentRequest = from_str(&serialized).expect("Failed to deserialize");
		
		assert_eq!(deserialized.slasher, slasher);
	}
}

#[test]
fn test_complex_nested_structure() {
	let complex_response = SlotInfoResponse {
		slots: vec![
			SlotInfo {
				slot: 1,
				offerings: vec![
					Offering { chain_id: 1, commitment_types: vec![1, 2, 3] },
					Offering { chain_id: 137, commitment_types: vec![] },
				],
			},
			SlotInfo {
				slot: 2,
				offerings: vec![],
			},
		],
	};
	
	let serialized = to_string(&complex_response).expect("Failed to serialize complex structure");
	let deserialized: SlotInfoResponse = from_str(&serialized).expect("Failed to deserialize complex structure");
	
	assert_eq!(deserialized.slots.len(), 2);
	assert_eq!(deserialized.slots[0].offerings.len(), 2);
	assert_eq!(deserialized.slots[0].offerings[0].commitment_types, vec![1, 2, 3]);
	assert!(deserialized.slots[0].offerings[1].commitment_types.is_empty());
	assert!(deserialized.slots[1].offerings.is_empty());
}

// Test Debug implementations
#[test]
fn test_debug_implementations() {
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![1, 2, 3],
		slasher: "0x1234".to_string(),
	};
	
	let debug_str = format!("{:?}", request);
	assert!(debug_str.contains("CommitmentRequest"));
	assert!(debug_str.contains("commitment_type"));
	assert!(debug_str.contains("payload"));
	assert!(debug_str.contains("slasher"));
}

#[test]
fn test_clone_implementations() {
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![1, 2, 3],
		slasher: "0x1234".to_string(),
	};
	
	let cloned_request = request.clone();
	assert_eq!(request.commitment_type, cloned_request.commitment_type);
	assert_eq!(request.payload, cloned_request.payload);
	assert_eq!(request.slasher, cloned_request.slasher);
}

// Test context types
#[test]
fn test_database_context_debug() {
	// We can't easily create a real DatabaseContext without a pool,
	// but we can test that the Debug implementation compiles
	use preconfirmation_gateway::types::DatabaseContext;
	use deadpool_postgres::{Config, Runtime};
	use tokio_postgres::NoTls;
	
	let mut cfg = Config::new();
	cfg.url = Some("postgresql://test:test@localhost/test".to_string());
	
	if let Ok(pool) = cfg.create_pool(Some(Runtime::Tokio1), NoTls) {
		let ctx = DatabaseContext::new(pool);
		let debug_str = format!("{:?}", ctx);
		assert!(debug_str.contains("DatabaseContext"));
	}
}