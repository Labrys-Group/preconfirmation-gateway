use jsonrpsee::{Extensions, types::Params};
use preconfirmation_gateway::{CommitmentRequest, DatabaseContext, RpcContext, commitment_request_handler, Config};
use serde_json::json;
use ethabi::{ParamType, encode, Token};
use tokio_postgres::NoTls;

// Create a mock context with custom validation config
fn create_mock_context(slasher_address: &str) -> RpcContext {
	// Create a mock database context (we don't need real DB for validation tests)
	let manager = deadpool_postgres::Manager::new(
		"postgresql://localhost/test".parse().unwrap(),
		NoTls,
	);
	let pool = deadpool_postgres::Pool::builder(manager).build().unwrap();
	let db_context = DatabaseContext::new(pool);

	// Create config with custom slasher address
	let mut config = Config::default();
	config.validation.slasher_address = slasher_address.to_string();

	RpcContext::new(db_context, config)
}

// Helper function to create a valid ABI-encoded InclusionPayload
fn create_valid_payload() -> Vec<u8> {
	let types = vec![
		ParamType::FixedBytes(32), // tx_hash (Bytes32)
		ParamType::Uint(256),      // nonce (uint256)
		ParamType::Uint(256),      // gas_limit (uint256)
		ParamType::Uint(64),       // slot (uint64)
	];

	let tokens = vec![
		Token::FixedBytes(vec![0x9f, 0xbb].into_iter().chain(std::iter::repeat(0).take(30)).collect()), // tx_hash
		Token::Uint(9.into()),           // nonce
		Token::Uint(500000.into()),      // gas_limit
		Token::Uint(1337.into()),        // slot
	];

	encode(&tokens)
}

// Helper function to create invalid payload (wrong structure)
fn create_invalid_payload() -> Vec<u8> {
	let types = vec![
		ParamType::Uint(256), // Wrong type - should be FixedBytes(32) for tx_hash
	];

	let tokens = vec![
		Token::Uint(42.into()),
	];

	encode(&tokens)
}

#[test]
fn test_commitment_request_valid() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_valid_payload(),
		slasher: slasher_address.to_string(),
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_ok());
	let signed_commitment = result.unwrap();
	assert_eq!(signed_commitment.commitment.commitment_type, 1);
	assert_eq!(signed_commitment.commitment.slasher, slasher_address);
}

#[test]
fn test_commitment_request_invalid_commitment_type() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	let request = CommitmentRequest {
		commitment_type: 2, // Invalid - should be 1
		payload: create_valid_payload(),
		slasher: slasher_address.to_string(),
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_err());
}

#[test]
fn test_commitment_request_commitment_type_zero() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	let request = CommitmentRequest {
		commitment_type: 0, // Invalid - should be 1
		payload: create_valid_payload(),
		slasher: slasher_address.to_string(),
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_err());
}

#[test]
fn test_commitment_request_invalid_payload_structure() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_invalid_payload(), // Invalid payload structure
		slasher: slasher_address.to_string(),
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_err());
}

#[test]
fn test_commitment_request_empty_payload() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: vec![], // Empty payload
		slasher: slasher_address.to_string(),
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_err());
}

#[test]
fn test_commitment_request_wrong_slasher_address() {
	let configured_slasher = "0x1234567890123456789012345678901234567890";
	let wrong_slasher = "0x9876543210987654321098765432109876543210";
	let context = create_mock_context(configured_slasher);

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_valid_payload(),
		slasher: wrong_slasher.to_string(), // Wrong slasher address
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_err());
}

#[test]
fn test_commitment_request_case_sensitive_slasher() {
	let configured_slasher = "0x1234567890123456789012345678901234567890";
	let uppercase_slasher = "0X1234567890123456789012345678901234567890"; // Different case
	let context = create_mock_context(configured_slasher);

	let request = CommitmentRequest {
		commitment_type: 1,
		payload: create_valid_payload(),
		slasher: uppercase_slasher.to_string(),
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	// Should fail because address comparison is case-sensitive
	assert!(result.is_err());
}

#[test]
fn test_commitment_request_malformed_params() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	// Test with malformed JSON parameters
	let invalid_params_json = json!(["invalid", "params"]);
	let params_string = invalid_params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());
	assert!(result.is_err());
}

#[test]
fn test_commitment_request_missing_fields() {
	let slasher_address = "0x1234567890123456789012345678901234567890";
	let context = create_mock_context(slasher_address);

	// Test with missing fields in the request
	let incomplete_request = json!({
		"commitment_type": 1,
		// Missing payload and slasher fields
	});
	let params_string = incomplete_request.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());
	assert!(result.is_err());
}

#[test]
fn test_commitment_request_all_validations_pass() {
	let slasher_address = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd";
	let context = create_mock_context(slasher_address);

	let request = CommitmentRequest {
		commitment_type: 1,                  // Valid
		payload: create_valid_payload(),     // Valid ABI-encoded InclusionPayload
		slasher: slasher_address.to_string(), // Matches config
	};

	let params_json = json!(request);
	let params_string = params_json.to_string();
	let params = Params::new(Some(&params_string));

	let result = commitment_request_handler(params, &context, &Extensions::new());

	assert!(result.is_ok());
	let signed_commitment = result.unwrap();

	// Verify the response structure
	assert_eq!(signed_commitment.commitment.commitment_type, 1);
	assert_eq!(signed_commitment.commitment.payload, create_valid_payload());
	assert_eq!(signed_commitment.commitment.slasher, slasher_address);
	assert!(!signed_commitment.signature.is_empty());
}