use std::sync::Arc;
use jsonrpsee::Extensions;
use jsonrpsee::core::RpcResult;
use tracing::{info, instrument};
use ethabi::{ParamType, decode};

use super::super::types::{Commitment, CommitmentRequest, FeeInfo, RpcContext, SignedCommitment, SlotInfoResponse};
use crate::crypto::{generate_request_hash, sign_commitment};

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

#[instrument(name = "commitment_request", skip(context, _extensions))]
pub async fn commitment_request_handler(
	params: jsonrpsee::types::Params<'static>,
	context: Arc<RpcContext>,
	_extensions: Extensions,
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
	if request.slasher != context.config.validation.slasher_address {
		return Err(jsonrpsee::types::error::ErrorCode::InvalidParams.into());
	}

	// Generate request hash
	let request_hash = generate_request_hash(&request)
		.map_err(|_| jsonrpsee::types::error::ErrorCode::InternalError)?;

	// Check if commitment already exists to prevent duplicates
	if context.database.commitment_exists(&request_hash).await
		.map_err(|_| jsonrpsee::types::error::ErrorCode::InternalError)? {
		return Err(jsonrpsee::types::error::ErrorCode::InvalidRequest.into());
	}

	// Create commitment with real request hash
	let commitment = Commitment {
		commitment_type: request.commitment_type,
		payload: request.payload,
		request_hash: request_hash.clone(),
		slasher: request.slasher,
	};

	// Sign the commitment
	let signature = sign_commitment(&commitment, &context.config.signing.private_key)
		.map_err(|_| jsonrpsee::types::error::ErrorCode::InternalError)?;

	let signed_commitment = SignedCommitment {
		commitment,
		signature,
	};

	// Save to database
	context.database.save_commitment(&signed_commitment).await
		.map_err(|_| jsonrpsee::types::error::ErrorCode::InternalError)?;

	info!("Commitment request processed and saved successfully");
	Ok(signed_commitment)
}

#[instrument(name = "commitment_result", skip(context, _extensions))]
pub async fn commitment_result_handler(
	params: jsonrpsee::types::Params<'static>,
	context: Arc<RpcContext>,
	_extensions: Extensions,
) -> RpcResult<SignedCommitment> {
	info!("Processing commitment result request");
	let request_hash: String = params.one()?;

	// Retrieve commitment from database
	match context.database.get_commitment_by_hash(&request_hash).await {
		Ok(Some(signed_commitment)) => {
			info!("Commitment result request processed successfully");
			Ok(signed_commitment)
		}
		Ok(None) => {
			info!("Commitment not found for hash: {}", request_hash);
			Err(jsonrpsee::types::error::ErrorCode::InvalidRequest.into())
		}
		Err(e) => {
			info!("Database error retrieving commitment: {}", e);
			Err(jsonrpsee::types::error::ErrorCode::InternalError.into())
		}
	}
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
