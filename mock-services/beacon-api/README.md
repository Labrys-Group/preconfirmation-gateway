# Mock Beacon API Server

A minimal mock implementation of the Ethereum Beacon Chain API for testing the preconfirmation gateway.

## What it does

Implements the Beacon API endpoints that the gateway expects:
- `GET /eth/v1/beacon/genesis` - Returns beacon chain genesis time
- `GET /eth/v1/beacon/headers/head` - Returns current slot based on wall clock time
- `GET /eth/v1/validator/duties/proposer/:epoch` - Returns proposer duties for an epoch
- `GET /eth/v1/beacon/blocks/:block_id` - Returns beacon block information
- `GET /eth/v1/node/health` - Health check endpoint

## Features

- **Real-time slot calculation**: Calculates current slot based on Ethereum mainnet genesis time (1606824023) and 12-second slot times
- **Deterministic proposer keys**: Returns consistent BLS public keys matching the mock-relay configuration
- **Full epoch support**: Returns proposer duties for all 32 slots in an epoch

## Setup

```bash
cd mock-services/beacon-api
npm install
```

## Run

```bash
npm start
```

The server runs on port 5051 (matching the gateway's beacon API configuration).

## Testing

```bash
# Get current head slot
curl http://localhost:5051/eth/v1/beacon/headers/head

# Get proposer duties for current epoch
CURRENT_EPOCH=$(date +%s | awk '{print int(($1 - 1606824023) / 384)}')
curl http://localhost:5051/eth/v1/validator/duties/proposer/$CURRENT_EPOCH

# Health check with slot info
curl http://localhost:5051/health
```

## Configuration

The mock server uses Ethereum mainnet parameters:
- Genesis time: 1606824023 (December 1, 2020)
- Seconds per slot: 12
- Slots per epoch: 32
- BLS proposer pubkey: `0x020202...` (48 bytes)
