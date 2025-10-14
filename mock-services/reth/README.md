# Mock Reth Server

A minimal mock implementation of an Ethereum execution client (Reth/Geth-compatible) for testing the preconfirmation gateway.

## What it does

Implements the JSON-RPC 2.0 endpoints that the gateway expects for gas price oracle:
- `eth_gasPrice` - Returns mock gas price (20 gwei)
- `eth_blockNumber` - Returns current block number (auto-increments)
- `eth_getBlockByNumber` - Returns block information
- `eth_chainId` - Returns chain ID (1 for mainnet)
- `net_version` - Returns network version
- `eth_syncing` - Returns sync status (always false)
- `web3_clientVersion` - Returns client version

## Features

- **JSON-RPC 2.0 compliant**: Standard request/response format
- **Auto-incrementing blocks**: Block number increases every 12 seconds
- **Realistic gas prices**: Returns 20 gwei (typical mainnet gas price)
- **Health endpoint**: Non-RPC endpoint for service health checks

## Setup

```bash
cd mock-services/reth
npm install
```

## Run

```bash
npm start
```

The server runs on port 8545 (standard Ethereum RPC port).

## Testing

```bash
# Get gas price
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'

# Get block number
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Health check
curl http://localhost:8545/health
```

## Configuration

Mock parameters:
- Gas price: 20 gwei (0x4a817c800 wei)
- Chain ID: 1 (Ethereum mainnet)
- Block time: 12 seconds
- Starting block: 19000000
