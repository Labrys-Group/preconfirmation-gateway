# Based Preconf Testnet Checklist

**Goal**: land an end-to-end preconf on a devnet/testnet that uses the URC, Constraints API, and Commitments API.

[Inclusion Preconf Specs](Based%20Preconf%20Testnet%20Checklist%2027798450aef480e08b01dc74107541c3/Inclusion%20Preconf%20Specs%2027798450aef4813198a1ddf7a05c70d9.md)

[How to properly sign messages for URC with your validator key?](Based%20Preconf%20Testnet%20Checklist%2027798450aef480e08b01dc74107541c3/How%20to%20properly%20sign%20messages%20for%20URC%20with%20your%20va%2027798450aef481798712ca7695bfa568.md)

### Preqreqs

- [ ]  Devnet deployed
- [ ]  Relay / Gateway / Builder live
- [ ]  Validators registered
- [ ]  URC deployed
- [ ]  Preconf protocol deployed

## End-to-end Flow

![image.png](Based%20Preconf%20Testnet%20Checklist%2027798450aef480e08b01dc74107541c3/image.png)

1. Proposer registers to URC 
    1. [signs `SignedRegistration` messages](https://github.com/eth-fabric/constraints-specs/blob/final-updates/specs/proposer.md#signing-and-submitting-a-registration)
    2. calls [script](https://github.com/eth-fabric/urc/tree/main/script#registering-to-the-urc) to register to URC
2. Proposer registers for PBS
    1. signs `ValidatorRegistration` message to produce `SignedValidatorRegistration` 
    2. calls [registerValidator](https://ethereum.github.io/builder-specs/#/Builder/registerValidator) endpoint
3. Proposer registers for preconf protocol
    1. [signs `Delegation` message to produce `SignedDelegation`](https://github.com/eth-fabric/constraints-specs/blob/final-updates/specs/proposer.md#signing-and-submitting-a-delegation)
    2. calls [postDelegate](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/postDelegate) endpoint
4. User requests preconf
    1. wallet calls [getDelegations](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getDelegations) to discover the Gateway
    2. wallet optionally queries a URC indexer to check proposer status
    3. calls [postCommitment](https://github.com/eth-fabric/commitments-specs/blob/commitment-request/specs/commitments-api.md#postcommitmentrequest) endpoint (Commitments API)
        
        ```python
        class CommitmentRequest(Container):
            # Type of commitment being requested
            commitment_type: uint64
            # Opaque input bytes used as part of the commitment
            payload: Bytes
            # Slasher contract for resolving commitment disputes
            slasher: Address
        
        class InclusionPayload(Container):
        		tx_hash: Bytes32
        		nonce: uint256
        		gas_limit: uint256
        		slot: uint64
        
        def inclusion_request(commitment: InclusionPayload, slasher: Address) -> CommitmentRequest:
        	return CommitmentRequest(
        		commitment_type: COMMITMENT_TYPE_INCLUSION, # 0x01
        		payload: abi.encode(commitment),
        		slasher: slasher)
        ```
        
    4. Gateway returns a `SignedCommitment`
        
        ```python
        class Commitment(Container):
            # The type of commitment being made
            commitment_type: uint64
            # Opaque payload bytes of the commitment
            payload: Bytes
            # Hash of the CommitmentRequest this Commitment is for
            request_hash: uint64
            # Slasher contract for resolving commitment disputes
            slasher: Address
        
        class SignedCommitment(Container):
            # The commitment message that was signed
            commitment: Commitment
            # The signature of the commitment message
            signature: ECDSASignature
            
        def get_signed_inclusion_commitment(request: CommitmentRequest, privkey: int) -> SignedCommitment:
        	assert request.commitment_type == COMMITMENT_TYPE_INCLUSION
        	
        	# note abi-encoded not SSZ
        	request_hash = keccak256(abi.encode(request))
        	
        	commitment = Commitment(
        		commitment_type: request.commitment_type,
        		payload: request.payload,
        		request_hash: request_hash,
        		slasher: request.slasher)
        		
        	# note abi-encoded not SSZ
        	commitment_hash = keccak256(abi.encode(commitment))
        	signature = ECDSA.sign(privkey, commitment_hash)
        	
        	return SignedCommitment(commitment: commmitment, signature: signature)
        ```
        
5. Gateway issues constraints
    1. creates one more `Constraint` messages
        
        ```python
        class Constraint(Container):
          constraint_type: uint64
          payload: Bytes
        
        # for inclusion preconfs, the commitment.payload == constraint.payload    
        class get_inclusion_constraint(commitment: Commitment) -> Constraint:
        	assert commitment.commitment_type == COMMITMENT_TYPE_INCLUSION
        	return Constraint(
        		constraint_type: commitment.commitment_type,
        		payload: commitment.payload)
        ```
        
    2. [creates a `SignedConstraints` object](https://github.com/eth-fabric/constraints-specs/blob/final-updates/specs/gateway.md#signing-constraints)
    3. calls [postConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/postConstraints)
6. Builder builds block
    1. calls [getConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getConstraints)
    2. [verifies Gateway signature](https://github.com/eth-fabric/constraints-specs/blob/final-updates/specs/builder.md#verify_constraint_signature)
    3. builds a valid block using constraints
    4. ASSUME OPTIMISTIC CASE: signs Gateway-signed `SignedConstraints`
        
        ```python
        def get_constraints_signature(constraints: SignedConstraints, privkey: int) -> BLSSignature:
            domain = compute_domain(DOMAIN_APPLICATION_BUILDER)
            signing_root = compute_signing_root(constraints, domain)
            return bls.Sign(privkey, signing_root)
        ```
        
    5. inserts the signature in the `SubmitBlockRequestWithProofs`
        
        ```python
        def get_submit_block_request_with_proofs(
            block: SubmitBlockRequest,
            constraints: SignedConstraints,
            privkey: int
        ) -> SubmitBlockRequestWithProofs:
        		# sign the Gateway-Signed
        		c_signature = get_constraints_signature(constraints, privkey)
        		
        		# create the proof
        		proof = ConstraintProofs([OPTION6_TYPE], [c_sig])
        
            return SubmitBlockRequestWithProofs(
              message: block.message,
        	    execution_payload: block.execution_payload,
        	    blobs_bundle: block.blobs_bundle,
        	    execution_requests: block.execution_requests,
        	    proofs: proof
        	    signature: block.signature
            )
        ```
        
    6. calls [submitBlockWithProofs](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/submitBlocksWithProofs)
7. Relay attests to block
    1. ASSUME OPTIMISTIC CASE: verifies `SignedBuilderBidWithProofs.proofs` includes a Builder-signed `SignedConstraints`
        
        ```python
        # this is specific to "option-5"
        def decode_signature_proof(proof: ConstraintProofs) -> BLSSignature:
        	assert len(proof.constraintTypes) == 1
        	assert len(proof.constraintTypes) == len(proof.payloads)
        	assert proof.constraintTypes[0] == OPTION6_TYPE
        	assert len(proof.payloads[0][2:]) == 96 * 2 # hex-encoded with '0x' prefix
        	return BLSSignature(bytes.fromhex(proof.payloads[0][2:]))
        
        # gateway-implementation
        def verify_block_request(signed_block: SubmitBlockRequest, constraints: SignedConstraints) -> bool:
            pubkey = signed_block.message.pubkey
            domain = compute_domain(DOMAIN_APPLICATION_BUILDER)
            signing_root = compute_signing_root(constraints, domain)
            signature = decode_signature_proof(signed_block.proofs)
            return bls.Verify(pubkey, signing_root, signature)
        ```
        
    2. assembles and signs a `BuilderBid`
8. Proposer gets header
    1. assembles and signs a `BlindedBeaconBlock`
    2. calls [submitBlindedBlock()](https://ethereum.github.io/builder-specs/#/Builder/submitBlindedBlock) to submit `SignedBlindedBeaconBlock` to Relay

## Checklist

### Commit-Boost

- [ ]  create separate repo for “delegation” module (name TBD)
- URC
    - [ ]  Can produce `SignedRegistration` messages using BLS key, [see specs](https://github.com/eth-fabric/constraints-specs/blob/main/specs/proposer.md#preparing-registrations)
        - [ ]  Use CB signing domain, mixed in with `0x00435255`
    - [x]  Ability to call `register()` on URC, [see script](https://github.com/eth-fabric/urc/tree/main/script#registering-to-the-urc)
- Unmodified PBS
    - [x]  Can produce  `SignedValidatorRegistration` message
    - [x]  Can call [registerValidator](https://ethereum.github.io/builder-specs/#/Builder/registerValidator) endpoint
    - [x]  Can call [getHeader](https://ethereum.github.io/builder-specs/#/Builder/getHeader) endpoint
- Constraints API
    - [ ]  Can set CB config to choose their Gateway
    - [ ]  Can sign `Delegation` message to produce `SignedDelegation`
        - [ ]  Use CB signing domain, mixed in with `0x0044656c`
        - [ ]  Slash protection logic for equivocating `SignedDelegation`
    - [ ]  Can call [postDelegate](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/postDelegate) endpoint
        - [ ]  Signing and posting should be done at the start of an epoch if they’re in the lookahead
    - [ ]  Can call [getDelegations](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getDelegations) endpoint
    - [ ]  Can call [getConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getConstraints) endpoint (low priority)

### Relay

- Unmodified PBS
    - [x]  Can respond to [registerValidator](https://ethereum.github.io/builder-specs/#/Builder/registerValidator) requests
    - [ ]  Can respond to getHeader requests
- Constraints API
    - [ ]  Can respond to [postDelegate](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/postDelegate) requests
    - [ ]  Can respond to [getDelegations](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getDelegations) requests
    - [ ]  Can respond to [postConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/postConstraints) requests
    - [ ]  Can respond to [getConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getConstraints) requests
    - [ ]  Can respond to [submitBlockWithProofs](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/submitBlocksWithProofs) requests
    - [ ]  Can respond to [getCapabilities](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getCapabilities) requests

### Gateway

- Constraints API
    - [ ]  Can call [getDelegations](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getDelegations) endpoint
    - [ ]  Can handle [postCommitment](https://github.com/eth-fabric/commitments-specs/blob/commitment-request/specs/commitments-api.md#postcommitmentrequest) requests
        - [ ]  Can handle [Inclusion Preconf](Based%20Preconf%20Testnet%20Checklist%2027798450aef480e08b01dc74107541c3/Inclusion%20Preconf%20Specs%2027798450aef4813198a1ddf7a05c70d9.md)
    - [ ]  Can call [postConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/postConstraints) endpoint
        - [ ]  Can handle [Inclusion Preconf](Based%20Preconf%20Testnet%20Checklist%2027798450aef480e08b01dc74107541c3/Inclusion%20Preconf%20Specs%2027798450aef4813198a1ddf7a05c70d9.md)
    - [ ]  Can call [getConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getConstraints) endpoint

### Builder

- Constraints API
    - [ ]  Can call [getConstraints](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getConstraints) endpoint
    - [ ]  Can call [submitBlockWithProofs](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/submitBlocksWithProofs) endpoint
        - [ ]  Can handle optimistic case
        - [ ]  Can build a correct block with inclusion preconfs
    - [ ]  [getDelegations](https://eth-fabric.github.io/constraints-specs/#/Constraints%20API/getDelegations) endpoint

### Potential considerations

- Circuit breaker in CB to stop delegating if `X` consecutive missed slots
- Check Gateway status before delegating to ensure they are up
- A way to temporarily disable local block building until after a delegation has expired
- if the `SignedConstraints.message.slot` has elapsed make `SignedConstraints` available via the Relay’s data api, otherwise it is only available via `GET /constraints` for those on `SignedConstraints.message.receivers` list