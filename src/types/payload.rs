use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// Import shared hex serde utilities
use crate::types::hex_serde as hex_bytes;

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
    #[serde(with = "hex_bytes")]
    pub signed_tx: Vec<u8>,
}

/// Execution preconfirmation payload (for future use)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPayload {
    /// The slot number for execution preconfirmation
    pub slot: u64,
    /// The transaction to execute
    #[serde(with = "hex_bytes")]
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
    /// Extracts the slot number from a commitment payload.
    ///
    /// The `commitment_type` determines how the `payload` is interpreted:
    /// - `1`: inclusion payload
    /// - `2`: execution payload
    /// An unknown `commitment_type` results in an error.
    ///
    /// # Returns
    ///
    /// `Ok(u64)` containing the extracted slot number on success, `Err` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// // JSON-encoded inclusion payload example
    /// let inclusion = InclusionPayload::new(42, vec![0x01, 0x02]);
    /// let payload = serde_json::to_vec(&inclusion).unwrap();
    /// let slot = PayloadParser::extract_slot(1, &payload).unwrap();
    /// assert_eq!(slot, 42);
    /// ```
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

    /// Parse an inclusion payload from raw bytes.
    ///
    /// Accepts either a JSON-encoded InclusionPayload or an RLP-encoded [slot, signed_tx] list and returns the parsed InclusionPayload.
    /// Returns an error if the payload cannot be parsed as either JSON or RLP.
    ///
    /// # Examples
    ///
    /// ```
    /// use crate::payloads::{InclusionPayload, PayloadParser};
    /// // JSON example
    /// let payload = serde_json::to_vec(&InclusionPayload::new(42, vec![1,2,3])).unwrap();
    /// let parsed = PayloadParser::parse_inclusion_payload(&payload).unwrap();
    /// assert_eq!(parsed.slot(), 42);
    /// ```
    pub fn parse_inclusion_payload(payload: &[u8]) -> Result<InclusionPayload> {
        // Try JSON parsing first (most flexible)
        if let Ok(parsed) = serde_json::from_slice::<InclusionPayload>(payload) {
            return Ok(parsed);
        }

        // Try RLP decoding for Ethereum-style encoding
        Self::parse_inclusion_payload_rlp(payload)
            .context("Failed to parse inclusion payload as both JSON and RLP")
    }

    /// Parses an execution preconfirmation payload from bytes.
    ///
    /// Attempts to decode the provided bytes as an `ExecutionPayload` serialized in JSON;
    /// returns an error if decoding fails.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = ExecutionPayload::new(42, vec![1,2,3], [0u8; 32]);
    /// let bytes = serde_json::to_vec(&payload).unwrap();
    /// let parsed = parse_execution_payload(&bytes).unwrap();
    /// assert_eq!(parsed.slot(), payload.slot());
    /// ```
    pub fn parse_execution_payload(payload: &[u8]) -> Result<ExecutionPayload> {
        // Try JSON parsing first
        if let Ok(parsed) = serde_json::from_slice::<ExecutionPayload>(payload) {
            return Ok(parsed);
        }

        // Fallback to RLP or other encoding
        Err(anyhow::anyhow!("Failed to parse execution payload"))
    }

    /// Extracts the slot number from an inclusion payload.
    ///
    /// Parses the provided payload as an inclusion payload (JSON first, then RLP) and returns its `slot`.
    ///
    /// # Returns
    ///
    /// The slot number contained in the parsed inclusion payload.
    ///
    /// # Examples
    ///
    /// ```
    /// // JSON-encoded inclusion payload with hex-serialized signed_tx
    /// let payload = br#"{"slot":42,"signed_tx":"0xdeadbeef"}"#;
    /// let slot = PayloadParser::extract_slot_from_inclusion_payload(payload).unwrap();
    /// assert_eq!(slot, 42);
    /// ```
    fn extract_slot_from_inclusion_payload(payload: &[u8]) -> Result<u64> {
        let inclusion_payload = Self::parse_inclusion_payload(payload)?;
        Ok(inclusion_payload.slot)
    }

    /// Extracts the slot number from an execution payload.
    ///
    /// Returns the slot number contained in the parsed execution payload.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = serde_json::to_vec(&ExecutionPayload::new(42, vec![], [0u8; 32])).unwrap();
    /// let slot = PayloadParser::extract_slot_from_execution_payload(&payload).unwrap();
    /// assert_eq!(slot, 42);
    /// ```
    fn extract_slot_from_execution_payload(payload: &[u8]) -> Result<u64> {
        let execution_payload = Self::parse_execution_payload(payload)?;
        Ok(execution_payload.slot)
    }

    /// Parses an RLP-encoded inclusion payload into an `InclusionPayload`.
    ///
    /// Expects the RLP to be a list of exactly two elements: `[slot, signed_tx]`.
    /// Returns an error if the RLP structure is invalid or if decoding of either
    /// field fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use rlp::RlpStream;
    /// // build RLP [slot, signed_tx]
    /// let mut s = RlpStream::new_list(2);
    /// s.append(&42u64);
    /// s.append(&b"\x01\x02\x03".to_vec());
    /// let bytes = s.out().to_vec();
    ///
    /// let parsed = crate::parse_inclusion_payload_rlp(&bytes).unwrap();
    /// assert_eq!(parsed.slot, 42);
    /// assert_eq!(parsed.signed_tx, b"\x01\x02\x03".to_vec());
    /// ```
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

    /// Serialize an `InclusionPayload` into JSON-encoded bytes.
    ///
    /// Returns the JSON representation of the payload as a `Vec<u8>`, or an error with context if encoding fails.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = InclusionPayload::new(42, vec![0x01, 0x02, 0x03]);
    /// let bytes = crate::payloads::PayloadParser::encode_inclusion_payload(&payload).unwrap();
    /// let decoded: InclusionPayload = serde_json::from_slice(&bytes).unwrap();
    /// assert_eq!(decoded.slot(), 42);
    /// ```
    pub fn encode_inclusion_payload(payload: &InclusionPayload) -> Result<Vec<u8>> {
        // Default to JSON encoding for flexibility
        serde_json::to_vec(payload)
            .context("Failed to encode inclusion payload as JSON")
    }

    /// Encodes an InclusionPayload as RLP bytes.
    ///
    /// Produces an RLP-encoded list containing the slot followed by the signed transaction bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = InclusionPayload::new(42, vec![0x01, 0x02]);
    /// let bytes = crate::encode_inclusion_payload_rlp(&payload).unwrap();
    /// assert!(!bytes.is_empty());
    /// ```
    pub fn encode_inclusion_payload_rlp(payload: &InclusionPayload) -> Result<Vec<u8>> {
        use rlp::RlpStream;

        let mut stream = RlpStream::new_list(2);
        stream.append(&payload.slot);
        stream.append(&payload.signed_tx);

        Ok(stream.out().to_vec())
    }
}

impl InclusionPayload {
    /// Constructs a new InclusionPayload with the given slot and signed transaction bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// let p = InclusionPayload::new(42, vec![1, 2, 3]);
    /// assert_eq!(p.slot(), 42);
    /// assert_eq!(p.signed_tx(), &[1, 2, 3][..]);
    /// ```
    pub fn new(slot: u64, signed_tx: Vec<u8>) -> Self {
        Self {
            slot,
            signed_tx,
        }
    }

    /// Retrieve the inclusion payload's slot number.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = InclusionPayload::new(42, vec![0x01]);
    /// assert_eq!(payload.slot(), 42);
    /// ```
    ///
    /// # Returns
    ///
    /// The slot number.
    pub fn slot(&self) -> u64 {
        self.slot
    }

    /// Reference to the payload's signed transaction bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// let p = InclusionPayload::new(42, vec![0xde, 0xad, 0xbe, 0xef]);
    /// assert_eq!(p.signed_tx(), &[0xde, 0xad, 0xbe, 0xef]);
    /// ```
    pub fn signed_tx(&self) -> &[u8] {
        &self.signed_tx
    }

    /// Validates the inclusion payload's fields.
    ///
    /// Returns an error if the signed transaction is empty or if the slot is zero.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    /// - `signed_tx` is empty.
    /// - `slot` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// #[test]
    /// fn validate_payload() {
    ///     let p = InclusionPayload::new(1, vec![0x01, 0x02]);
    ///     assert!(p.validate().is_ok());
    /// }
    /// ```
    pub fn validate(&self) -> Result<()> {
        if self.signed_tx.is_empty() {
            return Err(anyhow::anyhow!("Signed transaction cannot be empty"));
        }

        if self.slot == 0 {
            return Err(anyhow::anyhow!("Slot cannot be zero"));
        }

        Ok(())
    }

    /// Computes the Keccak-256 hash of the payload's `signed_tx`.
    ///
    /// Returns the 32-byte Keccak-256 hash of `signed_tx`. Returns an error if `signed_tx` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = InclusionPayload::new(1, vec![0x01, 0x02, 0x03]);
    /// let hash = payload.tx_hash().expect("hash computation failed");
    /// assert_eq!(hash.len(), 32);
    /// ```
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
    /// Creates a new `ExecutionPayload` with the provided slot, transaction bytes, and expected state root.
    ///
    /// # Examples
    ///
    /// ```
    /// let tx = vec![1u8, 2, 3];
    /// let root = [0u8; 32];
    /// let payload = ExecutionPayload::new(42, tx.clone(), root);
    /// assert_eq!(payload.slot, 42);
    /// assert_eq!(payload.transaction, tx);
    /// assert_eq!(payload.expected_state_root, root);
    /// ```
    pub fn new(slot: u64, transaction: Vec<u8>, expected_state_root: [u8; 32]) -> Self {
        Self {
            slot,
            transaction,
            expected_state_root,
        }
    }

    /// Retrieve the inclusion payload's slot number.
    ///
    /// # Examples
    ///
    /// ```
    /// let payload = InclusionPayload::new(42, vec![0x01]);
    /// assert_eq!(payload.slot(), 42);
    /// ```
    ///
    /// # Returns
    ///
    /// The slot number.
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