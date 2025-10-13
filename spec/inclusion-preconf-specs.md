# Inclusion Preconf Specs

### Constraints/Commitments API Refresher

**Resources**

- [Constraints Specs](https://github.com/eth-fabric/constraints-specs/tree/main/specs)
- [Commitment Specs](https://github.com/eth-fabric/commitments-specs/tree/main/specs)
- An example inclusion preconf slasher contract exists [here](https://github.com/eth-fabric/urc/blob/main/example/InclusionPreconfSlasher.sol) with tests [here](https://github.com/eth-fabric/urc/blob/main/test/InclusionPreconfSlasher.t.sol).

---

Users send the Gateway a [`CommitmentRequest`](https://github.com/eth-fabric/commitments-specs/blob/main/specs/commitments-api.md#commitmentrequest), containing a unique `commitment_type` identifier. The `payload` is encoded according to the `commitment_type` and the `slasher` is an execution address containing the logic to slash the proposer if the commitment is broken.

```python
# A CommitmentRequest message created by a user
class CommitmentRequest(Container):
    # Type of commitment being requested
    commitment_type: uint64
    # Opaque input bytes used as part of the commitment
    payload: Bytes
    # Slasher contract for resolving commitment disputes
    slasher: Address
```

The Gateway’s logic will decode the `payload` according to the `commitment_type`. Assuming they are accepting the request, they will hash the `CommitmentRequest` (ssz treehash) to populate the `request_hash` field and finally sign the `Commitment` with their ECDSA key to form the final `SignedCommitment`. Note the ECDSA key corresponds to the `SignedDelegation.message.committer` address from the validator’s delegation step.

```python
# A Commitment message responding to a CommitmentRequest
class Commitment(Container):
    # The type of commitment being made
    commitment_type: uint64
    # Opaque payload bytes of the commitment
    payload: Bytes
    # Hash of the CommitmentRequest this Commitment is for
    request_hash: Bytes32
    # Slasher contract for resolving commitment disputes
    slasher: Address

# A signed Commitment binding to a CommitmentRequest
class SignedCommitment(Container):
    # The commitment message that was signed
    commitment: Commitment
    # The signature of the commitment message
    signature: ECDSASignature
```

It’s the Gateway’s responsibility to create a matching [`Constraint`](https://github.com/eth-fabric/constraints-specs/blob/main/specs/constraints-api.md#endpoint-constraintsv0builderconstraints) for one or more `CommitmentRequest`. For example, a `SignedCommitment` may map to a single Gattaca `Frag` which encapsulates many `CommitmentRequest`.

Each `Constraint` has a unique `constraint_type`, that instructs the program how to decode a `payload`. It is not required for each `commitment_type` to map 1:1 with a `constraint_type` but for this document we will.

```python
# A constraint for transaction[s]
class Constraint(Container):
    constraint_type: uint64
    payload: Bytes
```

Multiple constraints are bundled into a `ConstraintsMessage` which is signed with a Gateway’s BLS key (i.e., the key the validator delegated to).

```python
# A signed "bundle" of constraints.
class SignedConstraints(Container):
    message: ConstraintsMessage
    signature: BLSSignature

# A "bundle" of constraints for a specific slot.
class ConstraintsMessage(Container):
    proposer: BLSPubkey
    delegate: BLSPubkey
    slot: uint64
    constraints: List[Constraint]
    receivers: List[BLSPubkey]
```

The Builder is required to submit a `ConstraintProofs` with their blocks. The payload could either be a simple signature (optimistic mode) or more complex like a Merkle inclusion proof. The proof is in the form of a `Bytes` payload, whose decoding and verifying logic is determined by the `constraint_type`.

```python
class VersionedSubmitBlockRequestWithProofs(Container):
    ... # All regular fields from VersionedSubmitBlockRequest, additionally
    proofs: ConstraintProofs

class ConstraintProofs(Container):
    constraintTypes: List[uint64, MAX_CONSTRAINTS_PER_SLOT]
    payloads: List[Bytes, MAX_CONSTRAINTS_PER_SLOT]
```

# Inclusion Preconf Spec

| Field | Value |
| --- | --- |
| `commitment_type` | `0x01` |
| `constraint_type` | `0x01` |

### InclusionPayload

This payload will be encoded as the `payload` and reused across the `CommitmentRequest`, `Commitment`, and `Constraint` containers.

```python
class InclusionPayload(Container):
		tx_hash: Bytes32
		nonce: uint256
		gas_limit: uint256
		slot: uint64
```

It should be encoded as follows:

```python
payload = InclusionPayload(tx_hash="0x9fbb...", nonce=9, gas_limit=500000, slot=1337)
encoded_payload = abi.encode(payload)
```

### InclusionProof (pessimistic)

```python
class InclusionProof(Container):
    tx_hash: Bytes32
    index: uint64
    merkle_hashes: List[Bytes32]
```

The builder would package multiple `InclusionProof` as follows:

```python
# example inclusion proofs
proof_0 = InclusionProof(
    tx_hash="0xcf8e...", index=7, merkle_hashes=["0xa7bc...", "0xd912...", ...]
).ssz_encode()

proof_1 = InclusionProof(
    tx_hash="0x9fbb...", index=9, merkle_hashes=["0xeeab...", "0x1a2c...", ...]
).ssz_encode()

# example envelope for multiple proofs
proofs = ConstraintProofs(
    constraintTypes=[0x01, 0x01],
    payloads=[
        proof_0,
        proof_1,
    ],
)
```

### InclusionProof (optimistic)

```python
class BuilderAttestationProof(Container):
		signed_constraints_hash: Bytes32
		signature: BLSSignature
```

If done optimistically, the builder would sign over the Gateway’s `SignedConstraints` to attest that their block satisfies all constraints.

```python
signed_constraints_hash = signed_constraints.hash_tree_root()
signature = bls.sign(signed_constraints_hash, privkey)
proof = BuilderAttestationProof(signed_constraints_hash, signature)
proofs = ConstraintProofs(
    constraintTypes=[0x01],
    payloads=[proof],
)
```

## Flows

- [ ]  User can create an `InclusionPayload` → encode → `CommitmentRequest` and send to Gateway
- [ ]  Gateway can decode `InclusionPayload` → `Commitment` → `SignedCommitment` and respond to User
- [ ]  Gateway can create a `Constraint` → `ConstraintsMessage` → `SignedConstraints` → send to Relay
- [ ]  Builder can request `SignedConstraints` → decode `InclusionPayload` → build block with `ConstraintProofs` → send to Relay
- [ ]  Relay can verify `ConstraintProofs`