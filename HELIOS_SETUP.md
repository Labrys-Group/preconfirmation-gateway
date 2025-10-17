# Helios Ethereum Light Node Setup

This document explains how to use the integrated Helios Ethereum light client with the preconfirmation gateway.

## What is Helios?

[Helios](https://github.com/a16z/helios) is a fast, secure, and portable Ethereum light client written in Rust by a16z. It provides:

- **Trustless verification**: Cryptographically verifies all Ethereum data using light client proofs
- **Fast sync**: Syncs in seconds instead of hours/days
- **Low resource usage**: Minimal CPU, memory, and storage requirements
- **Local RPC endpoint**: Provides a standard Ethereum JSON-RPC interface at `http://localhost:8545`

## Quick Start

### 1. Configure Environment Variables

Edit [.env.docker](.env.docker) and set your Execution RPC endpoint:

```bash
# REQUIRED: Set your Alchemy or Infura API key
HELIOS_EXECUTION_RPC=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY
```

**Important**: The execution RPC provider **must support `eth_getProof`**. Recommended providers:
- [Alchemy](https://www.alchemy.com/) - Supports `eth_getProof`
- [Infura](https://www.infura.io/) - Supports `eth_getProof`

### 2. Start Helios with Docker Compose

```bash
# Start all services including Helios
docker-compose up -d

# Or start only Helios
docker-compose up -d helios

# View Helios logs
docker-compose logs -f helios
```

Helios will:
1. Build the Docker image (first time only, takes ~5-10 minutes)
2. Sync with the Ethereum network (takes ~30-60 seconds)
3. Expose the JSON-RPC endpoint at `http://localhost:8545`

### 3. Verify Helios is Running

```bash
# Check if Helios is responding
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Expected response:
# {"jsonrpc":"2.0","result":"0x...","id":1}
```

### 4. Configure Gateway to Use Helios (Optional)

To use Helios as the RPC endpoint for the gateway's gas price oracle, update [config.toml](config.toml):

```toml
[reth]
# Point to Helios container (when running in Docker)
endpoint = "http://helios:8545"
```

Or set the environment variable:

```bash
export RETH_ENDPOINT=http://helios:8545
```

## Configuration Options

All Helios configuration is done via environment variables in [.env.docker](.env.docker):

### Required Configuration

| Variable | Description | Example |
|----------|-------------|---------|
| `HELIOS_EXECUTION_RPC` | Execution RPC endpoint (must support `eth_getProof`) | `https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY` |

### Optional Configuration

| Variable | Default | Options | Description |
|----------|---------|---------|-------------|
| `HELIOS_CONSENSUS_RPC` | `https://www.lightclientdata.org` | Any consensus node | Consensus layer endpoint |
| `HELIOS_NETWORK` | `mainnet` | `mainnet`, `sepolia`, `holesky` | Ethereum network to connect to |
| `HELIOS_CHECKPOINT` | (cached) | Beacon block hash | Weak subjectivity checkpoint |

### Example: Connecting to Sepolia Testnet

```bash
# .env.docker
HELIOS_EXECUTION_RPC=https://eth-sepolia.g.alchemy.com/v2/YOUR_API_KEY
HELIOS_NETWORK=sepolia
HELIOS_CHECKPOINT=0x...  # Optional: Get from https://sepolia.beaconcha.in/
```

## Architecture

The Docker Compose setup includes:

```
┌─────────────────────────────────────────────────┐
│  Docker Compose Stack                           │
│                                                 │
│  ┌──────────────┐    ┌─────────────────────┐  │
│  │   Gateway    │───▶│  Helios Light Node  │  │
│  │  (port 8080) │    │   (port 8545)       │  │
│  └──────────────┘    └─────────────────────┘  │
│         │                      │               │
│         │                      ▼               │
│         │            ┌──────────────────┐     │
│         │            │ Alchemy/Infura   │     │
│         │            │ (eth_getProof)   │     │
│         │            └──────────────────┘     │
│         ▼                                      │
│  ┌──────────────┐                             │
│  │  PostgreSQL  │                             │
│  │  (port 5432) │                             │
│  └──────────────┘                             │
└─────────────────────────────────────────────────┘
```

Key features:
- **Health checks**: Gateway waits for Helios to be healthy before starting
- **Persistent storage**: Checkpoint data persists across container restarts in `helios_data` volume
- **Network isolation**: All services communicate via Docker internal network

## Checkpoint Management

Helios uses **weak subjectivity checkpoints** as a trust anchor. After the first sync:

1. Helios caches the most recent finalized checkpoint in `/data/helios`
2. On subsequent starts, it uses the cached checkpoint automatically
3. The `helios_data` Docker volume persists this cache

### Manually Setting a Checkpoint

For faster first-time sync or to update an old checkpoint:

1. Get a recent finalized block hash from [beaconcha.in](https://beaconcha.in/)
2. Set in [.env.docker](.env.docker):
   ```bash
   HELIOS_CHECKPOINT=0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef
   ```
3. Restart Helios:
   ```bash
   docker-compose restart helios
   ```

## Testing and Development

### Using Helios Locally (without Docker)

If you want to run Helios outside Docker for development:

```bash
# Install heliosup
curl https://raw.githubusercontent.com/a16z/helios/master/heliosup/install | bash
heliosup

# Run Helios
helios ethereum \
  --execution-rpc https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY \
  --rpc-bind-ip 127.0.0.1 \
  --rpc-port 8545
```

Then update [config.toml](config.toml):
```toml
[reth]
endpoint = "http://localhost:8545"
```

### Testing RPC Methods

Helios implements most standard Ethereum RPC methods:

```bash
# Get latest block number
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Get block by number
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}'

# Get balance
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x...", "latest"],"id":1}'

# Get gas price
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'
```

## Troubleshooting

### Helios fails to start

**Check execution RPC supports `eth_getProof`:**
```bash
curl -X POST https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getProof","params":["0x0000000000000000000000000000000000000000",[],  "latest"],"id":1}'
```

If this fails, your RPC provider doesn't support `eth_getProof`. Switch to Alchemy or Infura.

### Checkpoint is too old

If you see warnings about old checkpoints:

1. Get a fresh checkpoint from https://beaconcha.in/
2. Set `HELIOS_CHECKPOINT` in [.env.docker](.env.docker)
3. Restart: `docker-compose restart helios`

### Slow sync or timeouts

Helios sync typically takes 30-60 seconds. If it's slower:

1. Check your internet connection
2. Try a different consensus RPC endpoint
3. Ensure your execution RPC has low latency
4. Check Helios logs: `docker-compose logs -f helios`

### Gateway can't connect to Helios

Ensure:
1. Helios is running: `docker-compose ps helios`
2. Helios is healthy: `docker-compose ps` (should show "healthy")
3. Config points to `http://helios:8545` (not `localhost` when in Docker)

## Performance Considerations

### Resource Usage

Helios is lightweight:
- **CPU**: Minimal (mostly idle after sync)
- **Memory**: ~100-200 MB
- **Disk**: ~10 MB for checkpoint cache
- **Network**: Initial sync downloads ~50-100 MB, then minimal

### Sync Time

- **First sync**: 30-60 seconds
- **Subsequent starts**: 5-10 seconds (uses cached checkpoint)
- **Stay synced**: Continuous, automatic

### RPC Performance

Helios provides similar performance to remote RPC providers:
- **Latency**: ~50-200ms per request (includes verification)
- **Throughput**: Hundreds of requests per second
- **Reliability**: No rate limits, local access

## Security Considerations

### Trust Model

Helios is a **trustless** light client:
- ✅ Verifies all data cryptographically using consensus proofs
- ✅ No trust in the execution RPC provider (only used for data, not trust)
- ✅ Trust only in the initial checkpoint (from beaconcha.in or Ethereum Foundation)

### Production Recommendations

1. **Checkpoint source**: Use checkpoints from trusted sources (beaconcha.in, Ethereum Foundation)
2. **Execution RPC**: Use reputable providers (Alchemy, Infura) for data availability
3. **Consensus RPC**: Use your own consensus node for maximum security
4. **Network isolation**: Keep Helios on internal network, not exposed to internet
5. **Monitoring**: Monitor sync status and health checks

## Additional Resources

- [Helios GitHub](https://github.com/a16z/helios)
- [Helios Documentation](https://github.com/a16z/helios/tree/master/docs)
- [Ethereum Light Client Specification](https://github.com/ethereum/consensus-specs/tree/dev/specs/altair/light-client)
- [beaconcha.in](https://beaconcha.in/) - Block explorer for checkpoints
- [Alchemy](https://www.alchemy.com/) - Recommended RPC provider
- [Infura](https://www.infura.io/) - Alternative RPC provider

## Support

For Helios-specific issues:
- [Helios GitHub Issues](https://github.com/a16z/helios/issues)
- [Helios Discord](https://discord.gg/a16z)

For preconfirmation gateway integration issues:
- Check [CLAUDE.md](CLAUDE.md) for project architecture
- Review [SYSTEM_OVERVIEW.md](SYSTEM_OVERVIEW.md) for implementation details
- Open an issue in this repository
