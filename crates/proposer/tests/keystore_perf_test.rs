//! Performance test for keystore decryption
//!
//! This test compares the performance of eth-keystore vs alloy's LocalSigner
//! for decrypting Ethereum keystores.
//!
//! Run with: cargo test --package proposer --test keystore_perf_test -- --nocapture

use std::time::Instant;

#[test]
fn test_eth_keystore_decryption_performance() {
	let keystore_path = "../../tests/data/keystores/keys/anvil-0";
	let password = "";

	println!("\n=== Testing eth-keystore crate ===");
	let start = Instant::now();

	let result = eth_keystore::decrypt_key(keystore_path, password);

	let duration = start.elapsed();

	match result {
		Ok(private_key) => {
			println!("✓ Successfully decrypted keystore");
			println!("✓ Private key length: {} bytes", private_key.len());
			println!("✓ Decryption time: {:?}", duration);

			if duration.as_secs() > 5 {
				println!("⚠ WARNING: Decryption took longer than 5 seconds!");
			} else if duration.as_millis() > 100 {
				println!("⚠ NOTICE: Decryption took over 100ms");
			}
		}
		Err(e) => {
			println!("✗ Failed to decrypt keystore: {}", e);
			panic!("Keystore decryption failed");
		}
	}

	// Assert reasonable performance (adjust threshold as needed)
	assert!(
		duration.as_secs() < 30,
		"Keystore decryption took too long: {:?}. This may indicate hanging.",
		duration
	);
}

#[cfg(feature = "alloy_keystore_comparison")]
#[test]
fn test_alloy_keystore_decryption_performance() {
	use alloy::signers::local::LocalSigner;
	use std::path::PathBuf;

	let keystore_path = PathBuf::from("../../tests/data/keystores/keys/anvil-0");
	let password = "";

	println!("\n=== Testing alloy LocalSigner::decrypt_keystore ===");
	let start = Instant::now();

	let result = LocalSigner::decrypt_keystore(&keystore_path, password);

	let duration = start.elapsed();

	match result {
		Ok(signer) => {
			println!("✓ Successfully decrypted keystore");
			println!("✓ Signer address: {:?}", signer.address());
			println!("✓ Decryption time: {:?}", duration);

			if duration.as_secs() > 5 {
				println!("⚠ WARNING: Decryption took longer than 5 seconds!");
			} else if duration.as_millis() > 100 {
				println!("⚠ NOTICE: Decryption took over 100ms");
			}
		}
		Err(e) => {
			println!("✗ Failed to decrypt keystore: {}", e);
			panic!("Keystore decryption failed");
		}
	}

	// Assert reasonable performance
	assert!(
		duration.as_secs() < 30,
		"Keystore decryption took too long: {:?}. This may indicate hanging.",
		duration
	);
}

#[test]
fn test_keystore_format_inspection() {
	use std::fs;

	let keystore_path = "../../tests/data/keystores/keys/anvil-0";

	println!("\n=== Inspecting keystore format ===");

	match fs::read_to_string(keystore_path) {
		Ok(contents) => {
			println!("✓ Keystore file readable");

			// Parse as JSON to extract KDF info
			if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
				if let Some(crypto) = json.get("crypto") {
					if let Some(kdf) = crypto.get("kdf") {
						println!("  KDF: {}", kdf.as_str().unwrap_or("unknown"));
					}

					if let Some(kdf_params) = crypto.get("kdfparams") {
						println!("  KDF Parameters:");

						if let Some(n) = kdf_params.get("n") {
							println!("    n (cost): {}", n);
							if let Some(n_val) = n.as_u64() {
								if n_val > 100000 {
									println!("    ⚠ HIGH COST PARAMETER - decryption will be slow!");
								}
							}
						}

						if let Some(r) = kdf_params.get("r") {
							println!("    r (block size): {}", r);
						}

						if let Some(p) = kdf_params.get("p") {
							println!("    p (parallelization): {}", p);
						}

						if let Some(dklen) = kdf_params.get("dklen") {
							println!("    dklen (derived key length): {}", dklen);
						}
					}
				}
			}
		}
		Err(e) => {
			println!("✗ Failed to read keystore file: {}", e);
			panic!("Cannot read keystore for inspection");
		}
	}
}

#[test]
fn test_detect_hanging_scenario() {
	use std::fs;
	use std::time::Duration;

	println!("\n=== Hang Detection Test ===");
	println!("This test simulates what users experience when decryption hangs.");

	let keystore_path = "../../tests/data/keystores/keys/anvil-0";
	let password = "";

	// Check file exists
	if !std::path::Path::new(keystore_path).exists() {
		println!("✗ Keystore file not found at: {}", keystore_path);
		panic!("Test keystore missing");
	}

	// Inspect parameters
	if let Ok(contents) = fs::read_to_string(keystore_path) {
		if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
			if let Some(kdf_params) = json
				.get("crypto")
				.and_then(|c| c.get("kdfparams"))
				.and_then(|kp| kp.get("n"))
				.and_then(|n| n.as_u64())
			{
				println!("Keystore scrypt n parameter: {}", kdf_params);

				// Estimate decryption time based on n parameter
				let estimated_ms = (kdf_params as f64 / 8192.0) * 50.0; // Rough estimate
				println!("Estimated decryption time: ~{:.0}ms", estimated_ms);

				if kdf_params > 500000 {
					println!("⚠ HIGH RISK: n > 500000, decryption may appear to hang!");
					println!("  Expected time: >30 seconds");
				} else if kdf_params > 100000 {
					println!("⚠ MEDIUM RISK: n > 100000, decryption will be slow");
					println!("  Expected time: 5-30 seconds");
				} else {
					println!("✓ LOW RISK: n <= 100000, should be reasonably fast");
				}
			}
		}
	}

	// Now actually decrypt with timeout simulation
	println!("\nAttempting decryption (with 60s timeout)...");
	let start = Instant::now();

	// Note: In a real async context, we'd use tokio::time::timeout
	// For this sync test, we just measure and report
	let result = eth_keystore::decrypt_key(keystore_path, password);
	let duration = start.elapsed();

	match result {
		Ok(_) => {
			println!("✓ Decryption succeeded in {:?}", duration);

			if duration > Duration::from_secs(10) {
				println!("⚠ HANG SYMPTOM: Users would perceive this as hanging!");
			}
		}
		Err(e) => {
			println!("✗ Decryption failed: {}", e);
		}
	}
}
