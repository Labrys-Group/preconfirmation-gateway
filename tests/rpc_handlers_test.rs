use anyhow::Result;
use jsonrpsee::{Extensions, types::Params};
use preconfirmation_gateway::{
	rpc::handlers::{commitment_request_handler, commitment_result_handler, fee_handler, slots_handler},
	types::{
		CommitmentRequest, DatabaseContext, RpcContext, Commitment, SignedCommitment, 
		FeeInfo, SlotInfoResponse
	},
};
use serde_json::json;
use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;

// Create a mock RPC context that doesn't require a real database connection
fn create_mock_rpc_context() -> RpcContext {
	// Create a minimal pool configuration that won't actually connect
	let mut cfg = Config::new();
	cfg.url = Some("postgresql://mock:mock@localhost:5432/mock_db".to_string());
	
	// Create pool - this won't actually connect until we try to use it
	let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)
		.expect("Failed to create mock pool");
	
	let db_context = DatabaseContext::new(pool);
	RpcContext::new(db_context)
}

#[test]
fn test_fee_handler_valid_request() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![1, 2, 3, 4, 5],
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	let params_json = json!([request]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = fee_handler(params, &context, &extensions);
	
	assert!(result.is_ok());
	let fee_info = result.unwrap();
	assert_eq!(fee_info.commitment_type, 1);
	assert_eq!(fee_info.fee_payload.len(), 32);
	assert_eq!(fee_info.fee_payload, vec![0u8; 32]);
}

#[test]
fn test_fee_handler_invalid_params() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	// Test with invalid JSON parameters
	let invalid_params_json = json!(["invalid", "params"]);
	let params_string = invalid_params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = fee_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_fee_handler_empty_params() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let params = Params::new(None);
	
	let result = fee_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_fee_handler_malformed_json() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let params_string = "invalid json";
	let params = Params::new(Some(params_string));
	
	let result = fee_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_fee_handler_missing_fields() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	// Request with missing fields
	let incomplete_request = json!([{"commitment_type": 1}]);
	let params_string = incomplete_request.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = fee_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_commitment_request_handler_valid() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let request = CommitmentRequest {
		commitment_type: 42,
		payload: vec![0xde, 0xad, 0xbe, 0xef],
		slasher: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
	};
	
	let params_json = json!([request]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_request_handler(params, &context, &extensions);
	
	assert!(result.is_ok());
	let signed_commitment = result.unwrap();
	
	// Verify the commitment structure
	assert_eq!(signed_commitment.commitment.commitment_type, 42);
	assert_eq!(signed_commitment.commitment.payload, vec![0xde, 0xad, 0xbe, 0xef]);
	assert_eq!(signed_commitment.commitment.slasher, "0xabcdef1234567890abcdef1234567890abcdef12");
	assert_eq!(signed_commitment.commitment.request_hash, "0x0000000000000000000000000000000000000000000000000000000000000000");
	
	// Verify signature format (should be hex string)
	assert_eq!(signed_commitment.signature.len(), 130); // 0x + 64 bytes = 130 chars
	assert!(signed_commitment.signature.starts_with("0x"));
}

#[test]
fn test_commitment_request_handler_invalid_params() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let params = Params::new(Some("invalid"));
	let result = commitment_request_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_commitment_result_handler_valid() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let request_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";
	let params_json = json!([request_hash]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_result_handler(params, &context, &extensions);
	
	assert!(result.is_ok());
	let signed_commitment = result.unwrap();
	
	// Verify the result structure
	assert_eq!(signed_commitment.commitment.commitment_type, 1);
	assert_eq!(signed_commitment.commitment.request_hash, request_hash);
	assert_eq!(signed_commitment.commitment.slasher, "0x0000000000000000000000000000000000000000");
	assert!(signed_commitment.commitment.payload.is_empty());
	
	// Verify signature format
	assert_eq!(signed_commitment.signature.len(), 130);
	assert!(signed_commitment.signature.starts_with("0x"));
}

#[test]
fn test_commitment_result_handler_invalid_params() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	// Test with array instead of single string
	let params_json = json!([["invalid", "array"]]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_result_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_commitment_result_handler_empty_params() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let params = Params::new(None);
	let result = commitment_result_handler(params, &context, &extensions);
	assert!(result.is_err());
}

#[test]
fn test_slots_handler() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	// Slots handler doesn't require parameters
	let params = Params::new(None);
	let result = slots_handler(params, &context, &extensions);
	
	assert!(result.is_ok());
	let slot_info = result.unwrap();
	
	// Currently returns empty slots array
	assert!(slot_info.slots.is_empty());
}

#[test]
fn test_slots_handler_with_params() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	// Even with parameters, should work (parameters are ignored)
	let params_json = json!(["ignored", "params"]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = slots_handler(params, &context, &extensions);
	assert!(result.is_ok());
}

// Test edge cases and validation

#[test]
fn test_large_payload_handling() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	// Test with large payload
	let large_payload = vec![0xFF; 10000]; // 10KB payload
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: large_payload.clone(),
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	let params_json = json!([request]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_request_handler(params, &context, &extensions);
	assert!(result.is_ok());
	
	let signed_commitment = result.unwrap();
	assert_eq!(signed_commitment.commitment.payload, large_payload);
}

#[test]
fn test_zero_commitment_type() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let request = CommitmentRequest {
		commitment_type: 0,
		payload: vec![],
		slasher: "0x0000000000000000000000000000000000000000".to_string(),
	};
	
	let params_json = json!([request]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_request_handler(params, &context, &extensions);
	assert!(result.is_ok());
	
	let signed_commitment = result.unwrap();
	assert_eq!(signed_commitment.commitment.commitment_type, 0);
}

#[test]
fn test_max_commitment_type() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let request = CommitmentRequest {
		commitment_type: u64::MAX,
		payload: vec![],
		slasher: "0x1234567890123456789012345678901234567890".to_string(),
	};
	
	let params_json = json!([request]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_request_handler(params, &context, &extensions);
	assert!(result.is_ok());
	
	let signed_commitment = result.unwrap();
	assert_eq!(signed_commitment.commitment.commitment_type, u64::MAX);
}

#[test]
fn test_empty_slasher_address() {
	let context = create_mock_rpc_context();
	let extensions = Extensions::new();
	
	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![1, 2, 3],
		slasher: "".to_string(),
	};
	
	let params_json = json!([request]);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));
	
	let result = commitment_request_handler(params, &context, &extensions);
	assert!(result.is_ok());
	
	let signed_commitment = result.unwrap();
	assert_eq!(signed_commitment.commitment.slasher, "");
}