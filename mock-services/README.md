# Mock Services for Integration Testing

This directory contains mock implementations of external services required by the preconfirmation gateway for end-to-end integration testing.

## Services

### 1. Mock Constraints API Relay (`../mock-relay/`)
- **Port**: 3501
- **Purpose**: Simulates the constraints API relay for delegation discovery and constraint submission
- **Endpoints**:
  - `GET /constraints/v1/delegations/:slot` - Returns mock delegations
  - `POST /constraints/v1/builder/constraints` - Accepts constraint submissions

### 2. Mock Beacon API (`beacon-api/`)
- **Port**: 5051
- **Purpose**: Simulates Ethereum Beacon Chain API for validator duties and timing
- **Endpoints**:
  - `GET /eth/v1/beacon/genesis` - Genesis information
  - `GET /eth/v1/beacon/headers/head` - Current beacon chain head
  - `GET /eth/v1/validator/duties/proposer/:epoch` - Proposer duties
  - `GET /eth/v1/beacon/blocks/:block_id` - Beacon blocks

### 3. Mock Reth (`reth/`)
- **Port**: 8545
- **Purpose**: Simulates Ethereum execution client for gas price oracle
- **Methods**:
  - `eth_gasPrice` - Returns mock gas price (20 gwei)
  - `eth_blockNumber` - Returns current block number
  - `eth_getBlockByNumber` - Returns block information
  - `eth_chainId`, `net_version`, `eth_syncing`, `web3_clientVersion`

## Quick Start

### Start All Services

```bash
./scripts/start-mock-services.sh
```

This will:
1. Start all three mock services
2. Install npm dependencies as needed
3. Wait for all services to be healthy
4. Output service URLs and log locations

### Stop All Services

```bash
./scripts/stop-mock-services.sh
```

### View Logs

```bash
# All logs are in the logs/ directory
tail -f logs/mock-relay.log
tail -f logs/mock-beacon-api.log
tail -f logs/mock-reth.log
```

## Service Configuration

### Mock Relay
- Committer address: `0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266` (hardhat account #0)
- BLS delegate key: `0x010101...` (48 bytes)
- BLS proposer key: `0x020202...` (48 bytes)
- Returns delegations for all requested slots

### Mock Beacon API
- Genesis time: 1606824023 (Ethereum mainnet)
- Slot duration: 12 seconds
- Slots per epoch: 32
- Real-time slot calculation based on wall clock

### Mock Reth
- Gas price: 20 gwei (0x4a817c800 wei)
- Chain ID: 1 (Ethereum mainnet)
- Block time: 12 seconds (auto-increments)

## Integration with Gateway

The gateway should be configured to use these mock services:

```toml
# config.toml
[beacon_api]
primary_endpoint = "http://localhost:5051"

[constraints_api]
relay_endpoint = "http://localhost:3501"

[reth]
endpoint = "http://localhost:8545"
```

Or via environment variables:

```bash
export BEACON_API_ENDPOINT="http://localhost:5051"
export CONSTRAINTS_API_ENDPOINT="http://localhost:3501"
export RETH_ENDPOINT="http://localhost:8545"
```

## Running Integration Tests

### Setup Test Keys

Integration tests require cryptographic keys to be provided via environment variables or a keys file. **Keys are never hardcoded in scripts.**

#### Option 1: Environment Variables

```bash
# Generate or provide your own test keys
export ECDSA_PRIVATE_KEY_1="<your_ecdsa_private_key>"  # 64 hex characters
export BLS_PRIVATE_KEY_1="<your_bls_private_key>"      # 64 hex characters
./scripts/integration-test.sh
```

**Generating Test Keys:**
```bash
# For ECDSA (must match mock relay address 0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266):
# Use Hardhat account #0, or generate with: cast wallet new

# For BLS:
openssl rand -hex 32
```

#### Option 2: Keys File (Recommended)

```bash
# Copy the example keys file
cp test-keys.sh.example test-keys.sh

# Edit test-keys.sh and replace placeholders with your actual keys
# The file contains instructions for generating keys
vim test-keys.sh

# Set the keys file path and run tests
export KEYS_FILE="test-keys.sh"
./scripts/integration-test.sh
```

**Important Notes:**
- The ECDSA key must correspond to address `0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266` to match the mock relay configuration
- **NEVER commit actual private keys to version control**
- `test-keys.sh` is in `.gitignore` and should never be committed
- For CI/CD, set keys as secrets in your pipeline configuration
- The example file (`test-keys.sh.example`) contains only placeholders, not real keys

### Run Tests

Once keys are configured, execute the full test suite:

```bash
./scripts/integration-test.sh
```

This will:
1. Verify PostgreSQL is running
2. Load and validate test keys
3. Start all mock services
4. Run database migrations
5. Build and start the gateway
6. Execute end-to-end tests
7. Clean up all services

## Manual Testing

You can also interact with the services manually:

```bash
# Check service health
curl http://localhost:3501/health
curl http://localhost:5051/health
curl http://localhost:8545/health

# Get current beacon slot
curl http://localhost:5051/eth/v1/beacon/headers/head

# Get gas price
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'

# Get delegations for a slot
curl http://localhost:3501/constraints/v1/delegations/12345678
```

## Development

Each mock service is a standalone TypeScript/Express application:

```bash
# Develop a specific service
cd mock-services/beacon-api
npm install
npm run dev  # Runs with watch mode
```

## Troubleshooting

### Port Already in Use

If you see "port already in use" errors:

```bash
# Find what's using the port
lsof -i :3501
lsof -i :5051
lsof -i :8545

# Kill the process
kill <PID>
```

### Service Won't Start

Check the logs for detailed error messages:

```bash
cat logs/mock-relay.log
cat logs/mock-beacon-api.log
cat logs/mock-reth.log
```

### Tests Failing

1. Ensure all services are healthy: `curl http://localhost:<PORT>/health`
2. Check gateway logs: `tail -f logs/gateway.log`
3. Verify database is accessible: `psql -c "SELECT 1" postgresql://postgres:postgres@localhost:5432/postgres`
4. Ensure environment variables are set correctly

## Architecture

```
┌─────────────┐
│   Gateway   │
│  (port 8080)│
└─────┬───────┘
      │
      ├──────────┐
      │          │
      │          ├─► Mock Beacon API (5051)
      │          │   - Current slot calculation
      │          │   - Proposer duties
      │          │
      │          ├─► Mock Reth (8545)
      │          │   - Gas price oracle
      │          │   - Block information
      │          │
      │          └─► Mock Relay (3501)
      │              - Delegation polling
      │              - Constraint submission
      │
      └─► PostgreSQL
          - Commitments storage
          - Delegations cache
```

## Testing Philosophy

These mock services enable **true integration testing** without requiring:
- Access to real Ethereum networks
- External API keys or credentials
- Complex infrastructure setup
- Network latency or rate limits

All responses are deterministic and fast, enabling rapid test iteration while still validating the complete system integration.
