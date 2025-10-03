use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Parser and utilities for commitment payloads
pub struct PayloadParser;

/// Structured representation of an inclusion preconfirmation payload
/// Matches the on-chain InclusionPayload struct:
/// - slot: uint64
/// - signed_tx: Bytes (full signed transaction, from which tx_hash/nonce/gas_limit are derivable)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InclusionPayload {
    /// The slot number for which inclusion is being preconfirmed
    pub slot: u64,
    /// The full signed transaction to be included (RLP-encoded Ethereum transaction)
    /// This can be decoded to extract tx_hash, nonce, gas_limit, and other fields
    pub signed_tx: Vec<u8>,
}

/// Execution preconfirmation payload (for future use)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPayload {
    /// The slot number for execution preconfirmation
    pub slot: u64,
    /// The transaction to execute
    pub transaction: Vec<u8>,
    /// Expected state root after execution
    pub expected_state_root: [u8; 32],
}

/// Generic payload wrapper that can handle different commitment types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommitmentPayload {
    /// Inclusion preconfirmation (commitment_type = 1)
    Inclusion(InclusionPayload),
    /// Execution preconfirmation (commitment_type = 2, for future use)
    Execution(ExecutionPayload),
    /// Unknown or custom payload type
    Raw(Vec<u8>),
}

impl PayloadParser {
    /// Extract slot number from a raw payload based on commitment type
    pub fn extract_slot(commitment_type: u64, payload: &[u8]) -> Result<u64> {
        match commitment_type {
            1 => Self::extract_slot_from_inclusion_payload(payload),
            2 => Self::extract_slot_from_execution_payload(payload),
            _ => Err(anyhow::anyhow!(
                "Unknown commitment type {} for slot extraction",
                commitment_type
            )),
        }
    }

    /// Parse an inclusion payload from raw bytes
    pub fn parse_inclusion_payload(payload: &[u8]) -> Result<InclusionPayload> {
        // Try JSON parsing first (most flexible)
        if let Ok(parsed) = serde_json::from_slice::<InclusionPayload>(payload) {
            return Ok(parsed);
        }

        // Try RLP decoding for Ethereum-style encoding
        Self::parse_inclusion_payload_rlp(payload)
            .context("Failed to parse inclusion payload as both JSON and RLP")
    }

    /// Parse an execution payload from raw bytes
    pub fn parse_execution_payload(payload: &[u8]) -> Result<ExecutionPayload> {
        // Try JSON parsing first
        if let Ok(parsed) = serde_json::from_slice::<ExecutionPayload>(payload) {
            return Ok(parsed);
        }

        // Fallback to RLP or other encoding
        Err(anyhow::anyhow!("Failed to parse execution payload"))
    }

    /// Extract slot from inclusion payload
    fn extract_slot_from_inclusion_payload(payload: &[u8]) -> Result<u64> {
        let inclusion_payload = Self::parse_inclusion_payload(payload)?;
        Ok(inclusion_payload.slot)
    }

    /// Extract slot from execution payload
    fn extract_slot_from_execution_payload(payload: &[u8]) -> Result<u64> {
        let execution_payload = Self::parse_execution_payload(payload)?;
        Ok(execution_payload.slot)
    }

    /// Parse inclusion payload using RLP encoding
    fn parse_inclusion_payload_rlp(payload: &[u8]) -> Result<InclusionPayload> {
        use rlp::Rlp;

        let rlp = Rlp::new(payload);

        // Expect RLP list with [slot, signed_tx]
        if !rlp.is_list() || rlp.item_count()? != 2 {
            return Err(anyhow::anyhow!("Invalid RLP structure for inclusion payload - expected [slot, signed_tx]"));
        }

        let slot: u64 = rlp.val_at(0)
            .context("Failed to decode slot from RLP")?;

        let signed_tx: Vec<u8> = rlp.val_at(1)
            .context("Failed to decode signed_tx from RLP")?;

        Ok(InclusionPayload {
            slot,
            signed_tx,
        })
    }

    /// Encode an inclusion payload to bytes
    pub fn encode_inclusion_payload(payload: &InclusionPayload) -> Result<Vec<u8>> {
        // Default to JSON encoding for flexibility
        serde_json::to_vec(payload)
            .context("Failed to encode inclusion payload as JSON")
    }

    /// Encode an inclusion payload to RLP bytes
    pub fn encode_inclusion_payload_rlp(payload: &InclusionPayload) -> Result<Vec<u8>> {
        use rlp::RlpStream;

        let mut stream = RlpStream::new_list(2);
        stream.append(&payload.slot);
        stream.append(&payload.signed_tx);

        Ok(stream.out().to_vec())
    }
}

impl InclusionPayload {
    /// Create a new inclusion payload
    pub fn new(slot: u64, signed_tx: Vec<u8>) -> Self {
        Self {
            slot,
            signed_tx,
        }
    }

    /// Get the slot number
    pub fn slot(&self) -> u64 {
        self.slot
    }

    /// Get the signed transaction bytes
    pub fn signed_tx(&self) -> &[u8] {
        &self.signed_tx
    }

    /// Validate the payload structure
    pub fn validate(&self) -> Result<()> {
        if self.signed_tx.is_empty() {
            return Err(anyhow::anyhow!("Signed transaction cannot be empty"));
        }

        if self.slot == 0 {
            return Err(anyhow::anyhow!("Slot cannot be zero"));
        }

        Ok(())
    }

    /// Decode the signed transaction to extract transaction hash
    /// This is a helper for deriving fields from the signed_tx
    pub fn tx_hash(&self) -> Result<[u8; 32]> {
        use tiny_keccak::{Hasher, Keccak};

        if self.signed_tx.is_empty() {
            return Err(anyhow::anyhow!("Cannot compute tx_hash of empty transaction"));
        }

        let mut hasher = Keccak::v256();
        let mut hash = [0u8; 32];
        hasher.update(&self.signed_tx);
        hasher.finalize(&mut hash);

        Ok(hash)
    }
}

impl ExecutionPayload {
    /// Create a new execution payload
    pub fn new(slot: u64, transaction: Vec<u8>, expected_state_root: [u8; 32]) -> Self {
        Self {
            slot,
            transaction,
            expected_state_root,
        }
    }

    /// Get the slot number
    pub fn slot(&self) -> u64 {
        self.slot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_slot_from_json_inclusion_payload() {
        let payload = InclusionPayload::new(12345, vec![1, 2, 3, 4]);
        let encoded = PayloadParser::encode_inclusion_payload(&payload).unwrap();

        let extracted_slot = PayloadParser::extract_slot(1, &encoded).unwrap();
        assert_eq!(extracted_slot, 12345);
    }

    #[test]
    fn test_extract_slot_from_rlp_inclusion_payload() {
        let payload = InclusionPayload::new(67890, vec![5, 6, 7, 8]);
        let encoded = PayloadParser::encode_inclusion_payload_rlp(&payload).unwrap();

        let extracted_slot = PayloadParser::extract_slot(1, &encoded).unwrap();
        assert_eq!(extracted_slot, 67890);
    }

    #[test]
    fn test_parse_inclusion_payload_json() {
        let payload = InclusionPayload::new(123, vec![0xaa, 0xbb, 0xcc]);
        let encoded = serde_json::to_vec(&payload).unwrap();

        let parsed = PayloadParser::parse_inclusion_payload(&encoded).unwrap();
        assert_eq!(parsed.slot, 123);
        assert_eq!(parsed.signed_tx, vec![0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn test_parse_inclusion_payload_rlp() {
        let payload = InclusionPayload::new(456, vec![0xdd, 0xee, 0xff]);
        let encoded = PayloadParser::encode_inclusion_payload_rlp(&payload).unwrap();

        let parsed = PayloadParser::parse_inclusion_payload(&encoded).unwrap();
        assert_eq!(parsed.slot, 456);
        assert_eq!(parsed.signed_tx, vec![0xdd, 0xee, 0xff]);
    }

    #[test]
    fn test_tx_hash_derivation() {
        let signed_tx = vec![0xf8, 0x6c, 0x01, 0x85, 0x04, 0xa8, 0x17, 0xc8, 0x00]; // Sample signed tx bytes
        let payload = InclusionPayload::new(789, signed_tx.clone());

        let tx_hash = payload.tx_hash().unwrap();
        assert_eq!(tx_hash.len(), 32); // Should be 32 bytes
    }

    #[test]
    fn test_payload_validation() {
        // Valid payload
        let valid_payload = InclusionPayload::new(100, vec![1, 2, 3]);
        assert!(valid_payload.validate().is_ok());

        // Invalid: empty tx data
        let invalid_payload = InclusionPayload::new(100, vec![]);
        assert!(invalid_payload.validate().is_err());

        // Invalid: zero slot
        let invalid_slot_payload = InclusionPayload::new(0, vec![1, 2, 3]);
        assert!(invalid_slot_payload.validate().is_err());
    }

    #[test]
    fn test_unknown_commitment_type() {
        let payload = vec![1, 2, 3, 4];
        let result = PayloadParser::extract_slot(999, &payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown commitment type"));
    }
}