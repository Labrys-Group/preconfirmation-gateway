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
	/// Parses this duty's BLS public key from a hex string and returns it as a 48-byte `BlsPublicKey`.
	///
	/// The method accepts an optional `0x` prefix, decodes the hex into bytes, and validates the result is exactly 48 bytes.
	///
	/// # Examples
	///
	/// ```ignore
	/// let duty = ValidatorDuty {
	///     validator_index: "0".to_string(),
	///     pubkey: format!("0x{}", "00".repeat(48)),
	///     slot: "0".to_string(),
	/// };
	/// let pk = duty.parse_pubkey().unwrap();
	/// assert_eq!(pk.0.len(), 48);
	/// ```ignore
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

	/// Parse the validator duty's slot field into an integer slot number.
	///
	/// Returns `Ok(u64)` containing the parsed slot number on success, or `Err(std::num::ParseIntError)` if the stored string is not a valid unsigned integer.
	///
	/// # Examples
	///
	/// ```ignore
	/// let vd = ValidatorDuty {
	///     validator_index: "0".to_string(),
	///     pubkey: "00".to_string(),
	///     slot: "123".to_string(),
	/// };
	/// let slot = vd.parse_slot().unwrap();
	/// assert_eq!(slot, 123);
	/// ```ignore
	pub fn parse_slot(&self) -> Result<u64, std::num::ParseIntError> {
		self.slot.parse::<u64>()
	}

	/// Parses the `validator_index` field into a `u64`.
	///
	/// Returns `Ok(u64)` containing the parsed validator index, or `Err(std::num::ParseIntError)`
	/// if the `validator_index` string is not a valid unsigned 64-bit integer.
	///
	/// # Examples
	///
	/// ```ignore
	/// let v = ValidatorDuty {
	///     validator_index: "123".to_string(),
	///     pubkey: "0x".to_string(),
	///     slot: "0".to_string(),
	/// };
	/// assert_eq!(v.parse_validator_index().unwrap(), 123u64);
	/// ```ignore
	pub fn parse_validator_index(&self) -> Result<u64, std::num::ParseIntError> {
		self.validator_index.parse::<u64>()
	}
}

/// Beacon chain timing utilities
pub struct BeaconTiming;

impl BeaconTiming {
	/// Converts a slot number to its corresponding epoch.
	///
	/// # Examples
	///
	/// ```ignore
	/// let epoch = slot_to_epoch(64);
	/// assert_eq!(epoch, 2);
	/// ```ignore
	pub fn slot_to_epoch(slot: u64) -> u64 {
		slot / timing::SLOTS_PER_EPOCH
	}

	/// Compute the first slot index of the given epoch.
	///
	/// # Examples
	///
	/// ```ignore
	/// let first = epoch_to_first_slot(0);
	/// assert_eq!(first, 0);
	///
	/// let first_epoch_one = epoch_to_first_slot(1);
	/// assert_eq!(first_epoch_one, timing::SLOTS_PER_EPOCH);
	/// ```ignore
	pub fn epoch_to_first_slot(epoch: u64) -> u64 {
		epoch * timing::SLOTS_PER_EPOCH
	}

	/// Compute the last slot index of a given epoch.
	///
	/// # Examples
	///
	/// ```ignore
	/// let last = epoch_to_last_slot(0);
	/// assert_eq!(last, timing::SLOTS_PER_EPOCH - 1);
	/// ```ignore
	pub fn epoch_to_last_slot(epoch: u64) -> u64 {
		(epoch + 1) * timing::SLOTS_PER_EPOCH - 1
	}

	/// Estimate the current beacon slot from the chain genesis time.
	///
	/// Returns the slot index computed from the difference between the current system time and `genesis_time`.
	/// If the current system time is before `genesis_time`, this returns `0`.
	///
	/// # Examples
	///
	/// ```ignore
	/// // When genesis is in the future, the estimated slot is 0.
	/// let future_genesis = std::time::SystemTime::now()
	///     .duration_since(std::time::UNIX_EPOCH)
	///     .unwrap()
	///     .as_secs() + 60;
	/// let slot = current_slot_estimate(future_genesis);
	/// assert_eq!(slot, 0);
	/// ```ignore
	pub fn current_slot_estimate(genesis_time: u64) -> u64 {
		let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		if now < genesis_time {
			return 0;
		}

		(now - genesis_time) / timing::SLOT_DURATION_SECONDS
	}

	/// Compute the number of seconds from the current system time until the start of a given slot.
	///
	/// The returned value is negative if the slot has already started.
	///
	/// # Parameters
	///
	/// - `genesis_time`: Unix epoch seconds when the chain genesis occurred.
	/// - `target_slot`: Slot number whose start time is being queried.
	///
	/// # Returns
	///
	/// `i64` number of seconds until the start of `target_slot`; negative if the slot start time is in the past.
	///
	/// # Examples
	///
	/// ```ignoreno_run
	/// let genesis = 1_700_000_000u64; // example genesis timestamp
	/// let slot = 10u64;
	/// let secs = time_until_slot(genesis, slot);
	/// println!("Seconds until slot {}: {}", slot, secs);
	/// ```ignore
	pub fn time_until_slot(genesis_time: u64, target_slot: u64) -> i64 {
		let slot_start_time = genesis_time + (target_slot * timing::SLOT_DURATION_SECONDS);
		let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

		slot_start_time as i64 - now as i64
	}

	/// Returns the Unix timestamp (seconds since epoch) of the deadline for submitting constraints for a given slot.
	///
	/// The deadline is computed relative to the provided genesis time.
	///
	/// # Examples
	///
	/// ```ignore
	/// let deadline = constraint_deadline_for_slot(0, 1);
	/// assert_eq!(deadline, timing::SLOT_DURATION_SECONDS + timing::CONSTRAINTS_SUBMISSION_DEADLINE);
	/// ```ignore
	pub fn constraint_deadline_for_slot(genesis_time: u64, slot: u64) -> u64 {
		genesis_time + (slot * timing::SLOT_DURATION_SECONDS) + timing::CONSTRAINTS_SUBMISSION_DEADLINE
	}

	/// Determines whether the current system time is within the constraint submission window for a slot.
	///
	/// # Arguments
	///
	/// * `genesis_time` - Unix timestamp (seconds) of chain genesis.
	/// * `slot` - Slot index to check the deadline for.
	///
	/// # Returns
	///
	/// `true` if the current system time is less than or equal to the constraint submission deadline for the given slot, `false` otherwise.
	///
	/// # Examples
	///
	/// ```ignore
	/// // Check the window for a slot given a far-future genesis time (deterministic example)
	/// let _ = BeaconTiming::is_within_constraint_window(10_000_000_000, 0);
	/// ```ignore
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
