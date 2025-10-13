# Preconfirmation

[Preconf Notes](https://www.notion.so/Preconf-Notes-263ced2a7e1380fbafaaf94465812f27?pvs=21) 

`commitmentRequest`

- validate `commitment_type` is `0x01`
- validate `payload` is of form:

```jsx
class InclusionPayload(Container):
		tx_hash: Bytes32
		nonce: uint256
		gas_limit: uint256
		slot: uint64
```

- validate `slasher` address matches some address specified in the `config.toml`
- sign commitment request with ECDSA and save to db
- add to array of constraints

```jsx
class Constraint(Container):
  constraint_type: uint64
  payload: Bytes
```

- post signed constraints to the relay every 12 seconds
- return signed commitment

`comitmentResult`

- find tx hash in db and return comitment

`slots`

- integrate with look-ahead window from relay / beacon node (?)
- chainid - hoodi
- only support commitment type 1

`fee`

- r eth node
- [For our pilot and sample pricing rule, what we can do is increase the marginal price of a new preconf as a function of the blockspace share already used.
E.g. this curve ensures there's always space left:
`tx_price = reth_oracle_price / (1 - (preconfed_gas / gas_limit)^k`](https://www.notion.so/For-our-pilot-and-sample-pricing-rule-what-we-can-do-is-increase-the-marginal-price-of-a-new-precon-264ced2a7e1380b58ff2c8a4d42c62fe?pvs=21)
    - k - some unknown value