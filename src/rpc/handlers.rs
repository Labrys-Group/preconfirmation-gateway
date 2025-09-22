use jsonrpsee::Extensions;
use jsonrpsee::core::RpcResult;
use tracing::{info, instrument};
use ethabi::{ParamType, decode};

use super::super::types::{Commitment, CommitmentRequest, FeeInfo, RpcContext, SignedCommitment, SlotInfoResponse};

fn validate_inclusion_payload(payload: &[u8]) -> Result<(), String> {
	// Define the ABI types for InclusionPayload struct:
	// tx_hash: Bytes32, nonce: uint256, gas_limit: uint256, slot: uint64
	let types = vec![
		ParamType::FixedBytes(32), // tx_hash (Bytes32)
		ParamType::Uint(256),      // nonce (uint256)
		ParamType::Uint(256),      // gas_limit (uint256)
		ParamType::Uint(64),       // slot (uint64)
	];

	match decode(&types, payload) {
		Ok(tokens) => {
			if tokens.len() != 4 {
				return Err("Invalid payload: expected 4 fields".to_string());
			}
			// Additional validation could be added here (e.g., check specific field values)
			Ok(())
		}
		Err(e) => Err(format!("Failed to decode InclusionPayload: {}", e)),
	}
}

#[instrument(name = "commitment_request", skip(_context, _extensions))]
pub fn commitment_request_handler(
	params: jsonrpsee::types::Params<'_>,
	_context: &RpcContext,
	_extensions: &Extensions,
) -> RpcResult<SignedCommitment> {
	info!("Processing commitment request");
	let request: CommitmentRequest = params.parse()?;

	// Validate commitment_type is 1
	if request.commitment_type != 1 {
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Validate payload is properly encoded InclusionPayload
	if let Err(_e) = validate_inclusion_payload(&request.payload) {
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Validate slasher address matches configured address
	if request.slasher != _context.config.validation.slasher_address {
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Database is now available via _context.database
	// Example usage: _context.database.with_client(|client| { /* database operations */ }).await?;
	// Or use the convenience method: _context.with_database(|client| { /* database operations */ }).await?;
	// Or get direct client access: _context.database_client();
	// TODO: Implement actual commitment logic
	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload,
		request_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
		slasher: request.slasher,
	};

	let signed_commitment = SignedCommitment {
		commitment,
		signature: "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".to_string(),
	};

	info!("Commitment request processed successfully");
	Ok(signed_commitment)
}

#[instrument(name = "commitment_result", skip(_context, _extensions))]
pub fn commitment_result_handler(
	params: jsonrpsee::types::Params<'_>,
	_context: &RpcContext,
	_extensions: &Extensions,
) -> RpcResult<SignedCommitment> {
	info!("Processing commitment result request");
	let request_hash: String = params.one()?;

	// TODO: Implement actual commitment retrieval logic
	let commitment = Commitment {
		commitment_type: 1,
		payload: vec![],
		request_hash,
		slasher: "0x0000000000000000000000000000000000000000".to_string(),
	};

	let signed_commitment = SignedCommitment {
		commitment,
		signature: "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".to_string(),
	};

	info!("Commitment result request processed successfully");
	Ok(signed_commitment)
}

#[instrument(name = "slots", skip(_context, _extensions))]
pub fn slots_handler(
	_params: jsonrpsee::types::Params<'_>,
	_context: &RpcContext,
	_extensions: &Extensions,
) -> RpcResult<SlotInfoResponse> {
	info!("Processing slots request");
	// TODO: Implement actual slots logic
	let response = SlotInfoResponse { slots: vec![] };

	info!("Slots request processed successfully");
	Ok(response)
}

#[instrument(name = "fee", skip(_context, _extensions))]
pub fn fee_handler(
	params: jsonrpsee::types::Params<'_>,
	_context: &RpcContext,
	_extensions: &Extensions,
) -> RpcResult<FeeInfo> {
	info!("Processing fee request");
	let request: CommitmentRequest = params.parse()?;

	// TODO: Implement actual fee calculation logic
	let fee_info = FeeInfo { fee_payload: vec![0u8; 32], commitment_type: request.commitment_type };

	info!("Fee request processed successfully");
	Ok(fee_info)
}
