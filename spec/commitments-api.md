# Commitments API Specification

## Abstract
The Commitments API is a set of endpoints for issuing proposer commitments.

## Motivation
The proposer commitments space is rapidly evolving, with preconfirmations representing just one early example. By providing a standardized Commitments API for users / applications / wallets to interact with commitment issuers, we can establish a unified interface that scales as new protocols enter with novel commitment types. This approach aims to prevent fragmentation and keeps complexity manageable as the ecosystem grows.

## API Goals
- Support both L1 and L2 commitments
- Support *generic* proposer commitments rather than just preconfs
- Simple wallet integration
- Support for users to learn about capabilities / fees

## Terminology
- **Commitment**: A binding and objectively verifiable promise made about a chain.
- **Proposer Commitment**: A commitment made by an L1 proposer, e.g., I promise to include transaction with hash `0x1234...` in an L1 block during slot 1000. 
- **Gateway**: The party with committment-issuing rights granted by the Proposer.
- **Slasher**: A smart contract containing logic to arbitrate disputes and slash collateral should commitments be broken. 

## API Scope
**In Scope**
- Requesting a commitment to be made
- Requesting to see a previously signed commitment
- Requesting to see a the upcoming capabilities of a Gateway
- Requesting fee information for a specific commitment request

**Out of Scope**

The [Constraints Specs](https://github.com/eth-fabric/constraints-specs) and [API](https://eth-fabric.github.io/constraints-specs/).


# API Summary

| **Namespace** | **Method** | **Endpoint** | **Description** |
| --- | --- | --- | --- |
| `commitments`   | `POST` | [/commitments/v0/gateway/commitmentRequest](commitments-api.md#postcommitmentrequest)        | Request a new `SignedCommitment`  |
| `commitments`   | `GET` | [/commitments/v0/gateway/commitmentResult](commitments-api.md#getcommitmentresult)        | Request an old `SignedCommitment` |
| `commitments`   | `GET` | [/commitments/v0/gateway/slots](commitments-api.md#slots)        | Get Gateway information for upcoming slots |
| `commitments`   | `POST` | [/commitments/v0/gateway/fee](commitments-api.md#getfeeinfo)        | Get commitment fee information |

# Schemas
### CommitmentRequest
```Python
# A CommitmentRequest message created by a user
class CommitmentRequest(Container):
    # Type of commitment being requested
    commitment_type: uint64
    # Opaque input bytes used as part of the commitment
    payload: Bytes
    # Slasher contract for resolving commitment disputes
    slasher: Address
```

### Commitment
```Python
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
```

### SignedCommitment
```Python
# A signed Commitment binding to a CommitmentRequest
class SignedCommitment(Container):
    # The commitment message that was signed
    commitment: Commitment
    # The signature of the commitment message
    signature: ECDSASignature
```

### SlotInfo
```Python
# Information about a Gateway's offerings at a specific slot
class SlotInfo(Container):
    # The L1 slot number 
    slot: Slot
    # The list of chain offerings
    offerings: list[Offering]
```

### SlotInfoResponse
```Python
# Response containing multiple SlotInfo
class SlotInfoResponse(Container):
    # A list of slot infos
    slots: list[SlotInfo]
```

### Offering
```Python
# Specifies which commitments can be made for a specific chain
class Offering(Container):
    # The id of the target chain
    chain_id: uint64
    # The types of commitments offered for the target chain
    commitment_types: list[uint64]
```

### FeeInfo
```Python
# Response body for fee information
class FeeInfo(Container):
    # Opaque bytes containing fee info related to the commitment type
    payload: Bytes
    # Type of commitment being requested
    commitment_type: uint64
```

# Endpoints

### **commitmentRequest**

Returns a new `SignedCommitment` for the given commitment request.

A `CommitmentRequest` contains an opaque `payload` bytes input that can be decoded according to the `commitment_type`. By making a request, the user / app / wallet is asking for the Gateway to make a commitment that is enforceable via the specified `slasher` contract.

Each `commitment_type` has its own rules for how a Gateway maps a `CommitmentRequest.payload` to a `Commitment.payload`. The `Commitment.request_hash` field is used to bind the `Commitment` to a specific `CommitmentRequest`, however this is not required to correspond 1:1. In the [appendix](commitments-api.md#example---l2-frag-commitment), we show how a `Commitment` can correspond to multiple `CommitmentRequest` containers by chaining their hashes.

The `SignedCommitment` response contains the ECDSA signature over the `Commitment`. The Commitments API doesn't specify the Gateway's key, but when used in conjunction with the Constraints API, the expectation is the key of the Gateway's `committer` address is used.

**Parameters**

- `commitment_type` (uint64): Type of commitment being requested
- `payload` (bytes): Opaque input bytes used as part of the commitment
- `slasher` (address): Slasher contract for resolving commitment disputes

**Returns**

`SignedCommitment` - A signed commitment containing the commitment and ECDSA signature


**Example Request**
```bash
curl -X POST --data '{"jsonrpc":"2.0","method":"commitmentRequest","params":[{"commitment_type":1,"payload":"0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef","slasher":"0x1234567890123456789012345678901234567890"}],"id":1}'
```

**Example Response**
```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "result": {
    "commitment": {
      "commitment_type": 1,
      "payload": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
      "request_hash": "0x1234567890123456789012345678901234567890000000000000000000000000",
      "slasher": "0x1234567890123456789012345678901234567890"
    },
    "signature": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1b"
  }
}
```

---

### **commitmentResult**

Returns a previously signed `SignedCommitment` for the given request hash.

When supplied with a valid `request_hash`, this endpoint responds with the `SignedCommitment` object containing the same `SignedCommitment.Commitment.request_hash`.

**Parameters**

- `request_hash` (bytes32): Hash of the CommitmentRequest to retrieve

**Returns**

`SignedCommitment` - A signed commitment containing the commitment and ECDSA signature

**Example Request**
```bash
curl -X POST --data '{"jsonrpc":"2.0","method":"commitmentResult","params":[{"request_hash":"0x1234567890123456789012345678901234567890000000000000000000000000"}],"id":1}'
```

**Example Response**
```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "result": {
    "commitment": {
      "commitment_type": 1,
      "payload": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
      "request_hash": "0x1234567890123456789012345678901234567890000000000000000000000000",
      "slasher": "0x1234567890123456789012345678901234567890"
    },
    "signature": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1b"
  }
}
```

---

### **slots**

Returns Gateway information for upcoming slots.

When called, the Gateway returns a `SlotInfo` for each upcoming L1 slot in the current or upcoming epoch. Each `SlotInfo` contains a list of `Offering` objects which specify the types of commitments they offer for a given chain, e.g., inclusion preconfs for the L1.

It should be noted that this endpoint does not provide guarantees that the Gateway is actually capable of providing these. For example, for proposer commitments that require delegations, the user should also consult the Constraints API to verify if the Gateway received delegations for the slot in question.

**Parameters**

None

**Returns**

`SlotInfoResponse` - A list of slot infos containing offerings for upcoming slots

**Example Request**
```bash
curl -X POST --data '{"jsonrpc":"2.0","method":"slots","params":[],"id":1}'
```

**Example Response**
```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "result": {
    "slots": [
      {
        "slot": 1000,
        "offerings": [
          {
            "chain_id": 1,
            "commitment_types": [1, 2]
          },
          {
            "chain_id": 10,
            "commitment_types": [1]
          }
        ]
      },
      {
        "slot": 1001,
        "offerings": [
          {
            "chain_id": 1,
            "commitment_types": [1, 2]
          },
          {
            "chain_id": 10,
            "commitment_types": [1]
          }
        ]
      }
    ]
  }
}
```

---

### **fee**

Returns commitment fee information for the given commitment request.

Since each proposer commitment protocol may have differing pricing mechanisms, i.e., per-request or subscription based, this endpoint is intentionally left generic. Users submit a `CommitmentRequest` and receive a `FeeInfo` object containing opaque `payload` bytes and a `commitment_type` to decode the `payload` into protocol-specific pricing information.

**Parameters**

- `commitment_type` (uint64): Type of commitment being requested
- `payload` (bytes): Opaque input bytes used as part of the commitment
- `slasher` (address): Slasher contract for resolving commitment disputes

**Returns**

`FeeInfo` - Fee information containing opaque payload and commitment type

**Example Request**
```bash
curl -X POST --data '{"jsonrpc":"2.0","method":"fee","params":[{"commitment_type":1,"payload":"0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef","slasher":"0x1234567890123456789012345678901234567890"}],"id":1}'
```

**Example Response**
```json
{
  "id": 1,
  "jsonrpc": "2.0",
  "result": {
    "payload": "0xfee1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
    "commitment_type": 1
  }
}
```

# Appendix
### Example - L1 Inclusion Commitment
```python
tx_request = {
    "tx_data": "0x12345...",
    "slot": 1000,
    "chain_id": 1
}

# Encode the tx_request to the opaque `payload` bytes 
commitment_request = CommitmentRequest(L1_INCLUSION, json.dumps(tx_request), SOME_SLASHER)

# Submit the commitment request to the gateway
jsonrpc_request = {
    "jsonrpc": "2.0",
    "method": "commitmentRequest",
    "params": [commitment_request],
    "id": 1
}

response = http_post(
    url="/commitmentRequest",
    headers={"Content-Type": "application/json"},
    body=jsonrpc_request
)

# Parse the response
jsonrpc_response = json.loads(response.body)
commitment_response = SignedCommitment.decode(jsonrpc_response["result"])
preconf_response = json.loads(commitment_response.commitment.payload)

# For an L1 inclusion preconf, CommitmentRequest.payload == Commitment.payload
assert preconf_response["tx_data"] == tx_request["tx_data"]
assert preconf_response["slot"] == tx_request["slot"] 
assert preconf_response["chain_id"] == tx_request["chain_id"]
```

### Example - Taiyi Preconf Service
The [Taiyi Preconf Service](https://app.swaggerhub.com/apis-docs/luban-0d2/commitment_apis/v1#/) API can neatly interoperate with the Commitments API. The `SubmitTransactionRequestTypeB` is one of their preconf types:

```Python
class SubmitTransactionRequestTypeB(Container):
    request_id: string
    transaction: string
```

The response type is called a `PreconfResponseData`:

```python
class PreconfResponseData(Container):
    request_id: string
    commitment: string
    sequence_num: uint64
```

`SubmitTransactionRequestTypeB` can be encoded as part of a `CommitmentRequest`:

```python
taiyi_request = SubmitTransactionRequestTypeB('0729a580-2240-11e6-9eb5-0002a5d5c51b', '0x12345...')

# Encode the protocol-specific type to the opaque `payload` bytes
commitment_request = CommitmentRequest(TAIYI_TYPE, taiyi_request.encode(), TAIYI_SLASHER)

# Submit the commitment request to the gateway
jsonrpc_request = {
    "jsonrpc": "2.0",
    "method": "commitmentRequest",
    "params": [commitment_request],
    "id": 1
}

response = http_post(
    url="/commitmentRequest",
    headers={"Content-Type": "application/json"},
    body=jsonrpc_request
)

# Parse the response into a PreconfResponseData
jsonrpc_response = json.loads(response.body)
commitment_response = SignedCommitment.decode(jsonrpc_response["result"])
preconf_response = PreconfResponseData.decode(commitment_response.commitment.payload)
```

### Example - L2 Frag Commitment
Execution preconfs commit to a chain's state after execution. Some low-latency preconf protocols batch together transactions into a single updated state called a "Frag." In this case, there isn't a mapping between a single `CommitmentRequest` and a `SignedCommitment`, rather a commitment will be for multiple requests. 

This example aims to demonstrate how this is still compatible with the Commitments API. Upon receiving each `CommitmentRequest`, the Gateway doesn't immediately respond with a commitment. Instead they apply each `CommitmentRequest.payload` to continuously build the Frag and return one `SignedCommitment` to all of the requesters. The final `SignedCommitment.commitment.request_hash` should be bound to all of the input `CommitmentRequest` objects.

```python
# ...

# We start with 10 CommitmentRequest objects
assert len(requests) == 10

responses = []

# Submit the commitment requests to the gateway
for r in requests:
    jsonrpc_request = {
        "jsonrpc": "2.0",
        "method": "commitmentRequest",
        "params": [r],
        "id": 1
    }
    responses.append(http_post(
        url="/commitmentRequest",
        headers={"Content-Type": "application/json"},
        body=jsonrpc_request))

# Assert all responses are identical
for r in responses[1:]:
    assert r == responses[0], "All responses should be identical"

# Parse the response
jsonrpc_response = json.loads(responses[0].body)
commitment_response = SignedCommitment.decode(jsonrpc_response["result"])
request_hash = commitment_response.commitment.request_hash

# The Commitment.request_hash is composed of the hashes of all the input `CommitmentRequest` objects
assert request_hash == hash([hash(r) for r in requests])
```