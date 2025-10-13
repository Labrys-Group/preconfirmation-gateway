# How to properly sign messages for URC with your validator key?

### Signatures today

Currently the URC has two domains:

```solidity
// registry.sol
bytes public constant REGISTRATION_DOMAIN_SEPARATOR = "0x00555243"; // "URC" in little endian
bytes public constant DELEGATION_DOMAIN_SEPARATOR = "0x0044656c"; // "Del" in little endian
```

When signing or verifying, `toMessagePoint` converts hashes the abi-encoded message bytes with one of the domains.

```solidity
/// @notice Converts a message to a G2 point
/// @param message Arbitrarylength byte string to be hashed with the domainSeparator
/// @param domainSeparator The domain separation tag
/// @return A point in G2
function toMessagePoint(bytes memory message, bytes memory domainSeparator)
    internal
    view
    returns (BLS.G2Point memory)
{
    return BLS.toG2(
        BLS.Fp2({ c0_a: 0, c0_b: 0, c1_a: 0, c1_b: keccak256(abi.encodePacked(domainSeparator, message)) })
    );
}
```

Here is `sign()` and `verify()`:

```solidity
/// @notice Signs a message
/// @param message Arbitrarylength byte string to be hashed with the domainSeparator
/// @param privateKey The private key to sign with
/// @param domainSeparator The domain separation tag
/// @return A signature in G2
function sign(bytes memory message, uint256 privateKey, bytes memory domainSeparator)
    internal
    view
    returns (BLS.G2Point memory)
{
    return mul(toMessagePoint(message, domainSeparator), _u(privateKey));
    
/// @notice Verifies a signature
/// @param message Arbitrarylength byte string to be hashed
/// @param signature The signature to verify
/// @param publicKey The public key to verify against
/// @param domainSeparator The domain separation tag
/// @return True if the signature is valid, false otherwise
function verify(
    bytes memory message,
    BLS.G2Point memory signature,
    BLS.G1Point memory publicKey,
    bytes memory domainSeparator
) public view returns (bool) {
    // Hash the message bytes into a G2 point
    BLS.G2Point memory messagePoint = toMessagePoint(message, domainSeparator);

    // Invoke the BLS.pairing check to verify the signature.
    BLS.G1Point[] memory g1Points = new BLS.G1Point[](2);
    g1Points[0] = NEGATED_G1_GENERATOR();
    g1Points[1] = publicKey;

    BLS.G2Point[] memory g2Points = new BLS.G2Point[](2);
    g2Points[0] = signature;
    g2Points[1] = messagePoint;

    return BLS.pairing(g1Points, g2Points);
}
```

Importantly, to avoid the requirement to implement SSZ-encoding in Solidity, the URC expects that `Delegation` and `Registration` messages to be abi-encoded:

```solidity
// Reconstruct registration message
bytes memory message = abi.encode(operator.data.owner);

// Verify registration signature, note the domain separator mixin
if (
    BLSUtils.verify(
        message, proof.registration.signature, proof.registration.pubkey, REGISTRATION_DOMAIN_SEPARATOR
    )
) {
    revert FraudProofChallengeInvalid();
}
```

```solidity
// Reconstruct Delegation message
bytes memory message = abi.encode(delegation.delegation);

// Verify it was signed by the registered BLS key
if (
    !BLSUtils.verify(message, delegation.signature, delegation.delegation.proposer, DELEGATION_DOMAIN_SEPARATOR)
) {
    revert DelegationSignatureInvalid();
}
```

### How Commit-Boost handles signing

Commit-Boost has a Signer API (`/signer/v1/request_signature`) that accepts an `object_root` and provides a BLS signature using the validator’s BLS **key (the `type: "consensus"`).

![[https://commit-boost.github.io/commit-boost-client/api/](https://commit-boost.github.io/commit-boost-client/api/)](How%20to%20properly%20sign%20messages%20for%20URC%20with%20your%20va%2027798450aef481798712ca7695bfa568/image.png)

[https://commit-boost.github.io/commit-boost-client/api/](https://commit-boost.github.io/commit-boost-client/api/)

The `COMMIT_BOOST_DOMAIN` is then automatically used as the domain to prevent you from accidentally equivocating on PoS.

```rust
pub fn sign_commit_boost_root(
    chain: Chain,
    secret_key: &BlsSecretKey,
    object_root: B256,
) -> BlsSignature {
    let domain = compute_domain(chain, COMMIT_BOOST_DOMAIN);
    let signing_root = compute_signing_root(object_root, domain);
    sign_message(secret_key, signing_root)
}
```

The `signing_root` is obtained as the hash tree root of a `SigningData` struct:

```rust
pub fn compute_signing_root(object_root: B256, signing_domain: B256) -> B256 {
    #[derive(Default, Debug, TreeHash)]
    struct SigningData {
        object_root: B256,
        signing_domain: B256,
    }

    let signing_data = SigningData { object_root, signing_domain };
    signing_data.tree_hash_root()
}
```

### Compatibility with the URC

We can keep abi-encoding `Registration` and `Delegation` messages and continue to use `toMessagePoint()`. The output of `toMessagePoint()` is equivalent to an `object_root` that contains the `REGISTRATION_DOMAIN_SEPARATOR` or `DELEGATION_DOMAIN_SEPARATOR` already mixed in. What’s needed is to add additional code to the URC to create a `SigningData` from this `object_root` and mix-in the `COMMIT_BOOST_DOMAIN`. 

Since `SigningData` is so simple, there’s no need for bespoke SSZ-encoding logic. The biggest lift will be for the “Delegation Module” (i.e., the module that handles signing registrations and delegations) to use ABI-encoding instead of SSZ when crafting the `object_root`.

```solidity
function _computeSigningRoot(bytes32 objectRoot) public pure returns (bytes32) {
		return sha256(abi.encodePacked(objectRoot, COMMIT_BOOST_DOMAIN_SEPERATOR);
}
```

### Todos

- [ ]  Add `SigningData` support to URC
- [ ]  Check signature generated from CB can be verified by URC
- [ ]  Update constraints API docs

# 🚧 Update…

- The post-audit updates to Commit-Boost are looking like:

![[https://github.com/Commit-Boost/commit-boost-client/blob/sigp-audit-fixes/docs/docs/res/img/prop_commit_tree.png](https://github.com/Commit-Boost/commit-boost-client/blob/sigp-audit-fixes/docs/docs/res/img/prop_commit_tree.png)](How%20to%20properly%20sign%20messages%20for%20URC%20with%20your%20va%2027798450aef481798712ca7695bfa568/image%201.png)

[https://github.com/Commit-Boost/commit-boost-client/blob/sigp-audit-fixes/docs/docs/res/img/prop_commit_tree.png](https://github.com/Commit-Boost/commit-boost-client/blob/sigp-audit-fixes/docs/docs/res/img/prop_commit_tree.png)

The idea is that each each Commit-Boost module has a unique module ID and the Commit-Boost signer maintains an internal mapping of module ID to `Signing Id`. This exists to prevent a malicious module from requesting signatures while pretending to be another module to get the user slashed. 

A `Nonce` field is added to be used (or not) by the module. 

The `Chain ID` field is self-explanatory and known directly by the module.  

The Signer API (`/signer/v1/request_signature`) is modified as follows:

![image.png](How%20to%20properly%20sign%20messages%20for%20URC%20with%20your%20va%2027798450aef481798712ca7695bfa568/image%202.png)

To allow URC registrations and Constraints API delegations to be agnostic to modules, while adhering to the Signer API, we’re opting not to leave the `Signer ID` to be user-defined. In other words, instead of setting the `Signer ID` to be the `REGISTRATION_DOMAIN_SEPARATOR`, we let the user supply their `Signer ID`. 

This means Taiko / EthGas / etc can have their own custom module that is compatible with the URC/Constraints API flow.

The changes to the URC would look like:

![image.png](How%20to%20properly%20sign%20messages%20for%20URC%20with%20your%20va%2027798450aef481798712ca7695bfa568/image%203.png)

[https://github.com/Commit-Boost/commit-boost-client/blob/2dfe96b8d45d9c2bb37f71d56130a066dec16ec8/crates/common/src/types.rs#L307](https://github.com/Commit-Boost/commit-boost-client/blob/2dfe96b8d45d9c2bb37f71d56130a066dec16ec8/crates/common/src/types.rs#L307)