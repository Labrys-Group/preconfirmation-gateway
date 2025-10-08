use super::delegation::BlsPublicKey;
use serde::{Deserialize, Serialize};

/// Beacon chain slot timing constants
pub mod timing {
	/// Ethereum slot duration in seconds
	pub const SLOT_DURATION_SECONDS: u64 = 12;
	/// Slots per epoch
	pub const SLOTS_PER_EPOCH: u64 = 32;
	/// Constraint submission deadline within slot (seconds)
	pub const CONSTRAINTS_SUBMISSION_DEADLINE: u64 = 8;
}

/// Validator duty information from Beacon API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorDuty {
	/// Validator index in beacon state
	pub validator_index: String,
	/// BLS public key of the validator
	pub pubkey: String,
	/// Slot number for the duty
	pub slot: String,
}

/// Response from Beacon API for proposer duties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposerDutiesResponse {
	/// Execution optimistic flag
	pub execution_optimistic: bool,
	/// Whether response is finalized
	pub finalized: bool,
	/// Array of proposer duties
	pub data: Vec<ValidatorDuty>,
}

/// Beacon chain state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeaconState {
	/// Current slot
	pub slot: u64,
	/// Current epoch
	pub epoch: u64,
}

/// Helper functions for beacon chain operations
impl ValidatorDuty {
	/// Parse BLS public key from hex string to fixed array
	pub fn parse_pubkey(&self) -> Result<BlsPublicKey, hex::FromHexError> {
		let pubkey_str = self.pubkey.strip_prefix("0x").unwrap_or(&self.pubkey);
		let bytes = hex::decode(pubkey_str)?;

		if bytes.len() != 48 {
			return Err(hex::FromHexError::InvalidStringLength);
		}

		let mut pubkey = [0u8; 48];
		pubkey.copy_from_slice(&bytes);
		Ok(BlsPublicKey(pubkey))
	}

	/// Parse slot number from string
	pub fn parse_slot(&self) -> Result<u64, std::num::ParseIntError> {
		self.slot.parse::<u64>()
	}

	/// Parse validator index from string
	pub fn parse_validator_index(&self) -> Result<u64, std::num::ParseIntError> {
		self.validator_index.parse::<u64>()
	}
}

/// Beacon chain timing utilities
pub struct BeaconTiming;

impl BeaconTiming {
	/// Calculate epoch from slot number
	pub fn slot_to_epoch(slot: u64) -> u64 {
		slot / timing::SLOTS_PER_EPOCH
	}

	/// Calculate first slot of an epoch
	pub fn epoch_to_first_slot(epoch: u64) -> u64 {
		epoch * timing::SLOTS_PER_EPOCH
	}

	/// Calculate last slot of an epoch
	pub fn epoch_to_last_slot(epoch: u64) -> u64 {
		(epoch + 1) * timing::SLOTS_PER_EPOCH - 1
	}

	/// Get current slot based on genesis time
	/// Note: This is a simplified calculation, production should use actual beacon state
	pub fn current_slot_estimate(genesis_time: u64) -> u64 {
		let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		if now < genesis_time {
			return 0;
		}

		(now - genesis_time) / timing::SLOT_DURATION_SECONDS
	}

	/// Calculate time until slot starts (in seconds)
	pub fn time_until_slot(genesis_time: u64, target_slot: u64) -> i64 {
		let slot_start_time = genesis_time + (target_slot * timing::SLOT_DURATION_SECONDS);
		let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		slot_start_time as i64 - now as i64
	}

	/// Calculate constraint submission deadline for a slot
	pub fn constraint_deadline_for_slot(genesis_time: u64, slot: u64) -> u64 {
		genesis_time + (slot * timing::SLOT_DURATION_SECONDS) + timing::CONSTRAINTS_SUBMISSION_DEADLINE
	}

	/// Check if we're still within the constraint submission window
	pub fn is_within_constraint_window(genesis_time: u64, slot: u64) -> bool {
		let deadline = Self::constraint_deadline_for_slot(genesis_time, slot);
		let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		now <= deadline
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_epoch_calculations() {
		assert_eq!(BeaconTiming::slot_to_epoch(0), 0);
		assert_eq!(BeaconTiming::slot_to_epoch(31), 0);
		assert_eq!(BeaconTiming::slot_to_epoch(32), 1);
		assert_eq!(BeaconTiming::slot_to_epoch(63), 1);
		assert_eq!(BeaconTiming::slot_to_epoch(64), 2);

		assert_eq!(BeaconTiming::epoch_to_first_slot(0), 0);
		assert_eq!(BeaconTiming::epoch_to_first_slot(1), 32);
		assert_eq!(BeaconTiming::epoch_to_first_slot(2), 64);

		assert_eq!(BeaconTiming::epoch_to_last_slot(0), 31);
		assert_eq!(BeaconTiming::epoch_to_last_slot(1), 63);
		assert_eq!(BeaconTiming::epoch_to_last_slot(2), 95);
	}

	#[test]
	fn test_pubkey_parsing() {
		let duty = ValidatorDuty {
			validator_index: "123".to_string(),
			pubkey:
				"0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
					.to_string(),
			slot: "456".to_string(),
		};

		let parsed_pubkey = duty.parse_pubkey().unwrap();
		assert_eq!(parsed_pubkey.0.len(), 48);

		// Verify parsing works without 0x prefix too
		let duty_no_prefix = ValidatorDuty {
			validator_index: "123".to_string(),
			pubkey: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
				.to_string(),
			slot: "456".to_string(),
		};

		let parsed_no_prefix = duty_no_prefix.parse_pubkey().unwrap();
		assert_eq!(parsed_pubkey, parsed_no_prefix);
	}
}
