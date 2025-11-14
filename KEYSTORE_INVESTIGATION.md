# Keystore Reading Performance Investigation

## Issue Summary

On the `eth-fabric-main-as-is` branch, keystore decryption appears to hang when attempting to read ECDSA keystores for URC registration and transaction signing.

## Root Cause Analysis

### Current Implementation

The codebase currently uses the `eth-keystore` crate (v0.5.0) for decrypting Ethereum keystores:

```rust
// Current approach in crates/proposer/src/urc_registration.rs
let private_key = eth_keystore::decrypt_key(keystore_path, password)?;
let signer = PrivateKeySigner::from_bytes(&B256::from_slice(&private_key))
    .context("Failed to create signer from private key")?;
```

**Used in 8 locations across the codebase:**
- `crates/proposer/src/cli.rs:181`
- `crates/proposer/src/urc_registration.rs:75, 123, 171, 217, 247, 289, 318`

### Why It Hangs

The `eth-keystore` crate uses the **scrypt** key derivation function (KDF) to decrypt keystores. Scrypt is intentionally computationally expensive to make brute-force attacks difficult.

**Scrypt parameters** (from test keystore `tests/data/keystores/keys/anvil-0`):
```json
{
  "kdf": "scrypt",
  "kdfparams": {
    "dklen": 32,
    "n": 8192,      // CPU/memory cost parameter
    "r": 8,         // Block size parameter
    "p": 1          // Parallelization parameter
  }
}
```

While the test keystore has relatively low parameters (n=8192), keystores created by tools like Geth, MetaMask, or hardware wallets often use **much higher values**:
- **Standard**: n=262144, r=8, p=1 (Geth default)
- **High security**: n=524288 or higher

Higher values of `n` exponentially increase computation time, leading to:
- **Seconds to minutes** for decryption on standard hardware
- **Appearance of hanging** during decryption

### Additional Issues with eth-keystore

1. **No async support**: `decrypt_key()` is synchronous and blocks the thread
2. **No progress indication**: No way to show decryption progress to users
3. **CPU-intensive on main thread**: Blocks the tokio runtime during decryption
4. **Limited optimization**: The crate hasn't been updated recently (last update 2021)

## Recommended Solution: Switch to Alloy's Built-in Keystore Support

### Why Alloy?

The project already uses Alloy v1.0.35 with the `"full"` and `"signer-local"` features. Alloy provides built-in keystore support through `LocalSigner::decrypt_keystore()`:

**Benefits:**
1. ✅ **Same scrypt implementation** but potentially better optimized
2. ✅ **Simpler API** - one function call instead of two steps
3. ✅ **Better maintained** - actively developed as part of Alloy ecosystem
4. ✅ **Type-safe** - returns a `LocalSigner` directly
5. ✅ **Reduced dependencies** - removes need for separate `eth-keystore` crate
6. ✅ **Consistent with codebase** - already using Alloy for all other crypto operations

### Implementation Changes

#### 1. Enable Keystore Feature in Cargo.toml

Add the `keystore` feature to alloy dependencies:

```toml
[workspace.dependencies]
alloy = { version = "^1.0.35", features = [
    "full",
    "getrandom",
    "node-bindings",
    "providers",
    "rpc-types-beacon",
    "serde",
    "signer-local",
    "ssz",
    "keystore",  # ADD THIS
] }
```

#### 2. Update Import Statements

```rust
// OLD
use alloy::signers::local::PrivateKeySigner;
// + eth-keystore crate

// NEW
use alloy::signers::local::LocalSigner;
// No additional imports needed!
```

#### 3. Replace Keystore Decryption Logic

**OLD APPROACH** (2 steps):
```rust
let private_key = eth_keystore::decrypt_key(keystore_path, password)?;
let signer = PrivateKeySigner::from_bytes(&B256::from_slice(&private_key))
    .context("Failed to create signer from private key")?;
```

**NEW APPROACH** (1 step):
```rust
let signer = LocalSigner::decrypt_keystore(keystore_path, password)
    .context("Failed to decrypt keystore")?;
```

### Complete Example

From Alloy examples repository:

```rust
use alloy::signers::local::LocalSigner;
use std::path::PathBuf;

// Decrypt keystore and create signer in one step
let keystore_path = PathBuf::from("/path/to/keystore.json");
let password = "my-secure-password";
let signer = LocalSigner::decrypt_keystore(keystore_path, password)?;

// Use directly with provider
let wallet = EthereumWallet::from(signer);
let provider = ProviderBuilder::new()
    .wallet(wallet)
    .connect_http(rpc_url.parse()?);
```

## Alternative Solutions (If Alloy Doesn't Solve Performance)

### Option 1: Async Keystore Decryption

Move keystore decryption to a separate tokio task to avoid blocking:

```rust
let keystore_path = keystore_path.to_owned();
let password = password.to_owned();

let signer = tokio::task::spawn_blocking(move || {
    LocalSigner::decrypt_keystore(&keystore_path, &password)
}).await??;
```

**Benefits:**
- Doesn't block the tokio runtime
- Allows other async work to continue
- Better for CLI tools

### Option 2: Add Progress Indication

For long-running decryption, show progress to users:

```rust
info!("Decrypting keystore... This may take 10-30 seconds depending on keystore security settings.");

let signer = tokio::task::spawn_blocking(move || {
    LocalSigner::decrypt_keystore(&keystore_path, &password)
}).await.context("Keystore decryption task failed")??;

info!("✓ Keystore decrypted successfully");
```

### Option 3: Cache Decrypted Keys (Use With Caution)

For development/testing only - cache decrypted keys in memory:

```rust
// WARNING: Security implications! Only for dev/testing
lazy_static! {
    static ref SIGNER_CACHE: Arc<RwLock<HashMap<String, LocalSigner>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

async fn get_or_decrypt_signer(
    keystore_path: &str,
    password: &str
) -> Result<LocalSigner> {
    let cache = SIGNER_CACHE.read().await;
    if let Some(signer) = cache.get(keystore_path) {
        return Ok(signer.clone());
    }
    drop(cache);

    // Decrypt in background
    let signer = tokio::task::spawn_blocking(/* ... */).await??;

    let mut cache = SIGNER_CACHE.write().await;
    cache.insert(keystore_path.to_string(), signer.clone());
    Ok(signer)
}
```

⚠️ **DO NOT use caching in production** - keys should not be kept in memory longer than necessary.

## Implementation Plan

### Phase 1: Switch to Alloy (Immediate)

1. ✅ Add `"keystore"` feature to workspace Cargo.toml
2. ✅ Update `crates/proposer/Cargo.toml` to remove `eth-keystore` dependency
3. ✅ Replace all 8 occurrences of `eth_keystore::decrypt_key()` with `LocalSigner::decrypt_keystore()`
4. ✅ Update import statements
5. ✅ Test with existing keystores

**Files to modify:**
- `Cargo.toml` (workspace)
- `crates/proposer/Cargo.toml`
- `crates/proposer/src/cli.rs`
- `crates/proposer/src/urc_registration.rs`

### Phase 2: Add Async + Progress (If Still Slow)

1. Wrap keystore decryption in `spawn_blocking()`
2. Add informative log messages
3. Test with high-parameter keystores

### Phase 3: Performance Testing

Create a test suite to compare:
- Time to decrypt test keystores (n=8192)
- Time to decrypt standard keystores (n=262144)
- Memory usage during decryption

## Testing Strategy

### Test 1: Verify Functionality
```bash
# Test with low-parameter keystore (should be fast)
./proposer register \
  --urc-address 0x... \
  --keystore tests/data/keystores/keys/anvil-0 \
  --password "" \
  --dry-run
```

### Test 2: Measure Performance
```rust
#[tokio::test]
async fn test_keystore_decryption_performance() {
    let start = std::time::Instant::now();
    let signer = LocalSigner::decrypt_keystore(
        "tests/data/keystores/keys/anvil-0",
        ""
    ).unwrap();
    let duration = start.elapsed();

    println!("Decryption took: {:?}", duration);
    assert!(duration < std::time::Duration::from_secs(5),
        "Keystore decryption took too long: {:?}", duration);
}
```

### Test 3: High-Parameter Keystore
Create a test keystore with realistic parameters (n=262144) and verify:
- Decryption completes successfully
- Time is reasonable (<30 seconds)
- No hanging or timeout issues

## Expected Outcomes

### Optimistic Scenario
- ✅ Switching to Alloy's `LocalSigner::decrypt_keystore()` solves the hanging
- ✅ Decryption time is acceptable (<5 seconds for n=8192, <30 seconds for n=262144)
- ✅ No additional changes needed

### Realistic Scenario
- ⚠️ Alloy decryption is the same speed (scrypt is still slow)
- ✅ But using `spawn_blocking()` prevents blocking the runtime
- ✅ Adding progress logs improves user experience
- ✅ No actual "hanging" - just user perception improved

### Pessimistic Scenario
- ❌ Scrypt is fundamentally slow with high parameters
- ✅ Document expected decryption times for users
- ✅ Add `--keystore-timeout` flag with clear error messages
- ✅ Recommend users create low-parameter keystores for CLI usage
- ✅ Consider alternative keystore formats (e.g., encrypted JSON with AES-GCM)

## References

- [Alloy LocalSigner Documentation](https://docs.rs/alloy-signer-local/latest/alloy_signer_local/struct.LocalSigner.html)
- [Alloy Keystore Example](https://github.com/alloy-rs/examples/blob/main/examples/wallets/examples/keystore_signer.rs)
- [eth-keystore Crate](https://crates.io/crates/eth-keystore)
- [Scrypt RFC 7914](https://www.rfc-editor.org/rfc/rfc7914)
- [Ethereum Keystore Format (Web3 Secret Storage)](https://ethereum.org/en/developers/docs/data-structures-and-encoding/web3-secret-storage/)

## Next Steps

1. Review this investigation document
2. Approve implementation plan
3. Implement Phase 1 (switch to Alloy)
4. Test and measure results
5. Proceed to Phase 2/3 only if needed
