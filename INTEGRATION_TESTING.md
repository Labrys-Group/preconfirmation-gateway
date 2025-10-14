# Integration Testing Guide

This document provides a comprehensive guide to running end-to-end integration tests for the preconfirmation gateway using mock services.

## Overview

The integration test suite validates the complete gateway workflow without requiring external services:

- ✅ Mock Beacon API (port 5051)
- ✅ Mock Reth execution client (port 8545)
- ✅ Mock Constraints API relay (port 3501)
- ✅ PostgreSQL database
- ✅ Gateway RPC server (port 8080)

## Quick Start

### Prerequisites

1. **PostgreSQL** running on `localhost:5432`
2. **Node.js** (for mock services)
3. **Rust & Cargo** (for gateway)
4. **Environment variables** configured (see below)

### Run Integration Tests

```bash
# One command to run the full test suite
./scripts/integration-test.sh
```

This script will:
1. Check PostgreSQL connectivity
2. Start all mock services
3. Run database migrations
4. Build and start the gateway
5. Execute end-to-end tests
6. Clean up all services

## Manual Testing

### 1. Start Mock Services

```bash
./scripts/start-mock-services.sh
```

This starts:
- Mock Relay (port 3501)
- Mock Beacon API (port 5051)
- Mock Reth (port 8545)

Logs are written to `./logs/`

### 2. Configure Environment

```bash
export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/preconfirmation_gateway"
export BEACON_API_ENDPOINT="http://localhost:5051"
export RETH_ENDPOINT="http://localhost:8545"

# Test keys (matching mock services)
export ECDSA_PRIVATE_KEY_1="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
export BLS_PRIVATE_KEY_1="0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001"
```

### 3. Run Migrations

```bash
sqlx migrate run
```

### 4. Start Gateway

```bash
cargo run --release
```

### 5. Run E2E Tests

```bash
cd tests/e2e
npm test
```

### 6. Stop Services

```bash
./scripts/stop-mock-services.sh
```

## Test Coverage

The integration tests validate:

### Service Health
- ✓ All mock services respond to health checks
- ✓ Gateway starts successfully
- ✓ Database migrations complete

### RPC Endpoints
- ✓ `slots` - Returns 64 future slots with Hoodi chain offerings
- ✓ `fee` - Returns dynamic fee calculation
- ✓ `commitmentRequest` - Accepts commitments and returns ECDSA signature
- ✓ `commitmentResult` - Retrieves stored commitments by hash

### Error Handling
- ✓ Rejects invalid commitment types
- ✓ Returns null for non-existent commitments
- ✓ Validates payload format

### Background Services
- ✓ Delegation polling discovers authorities from relay
- ✓ Constraint submission converts commitments to constraints
- ✓ BLS signing for constraint messages

## Testing Specific Flows

### Test a Commitment Request

```bash
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "commitmentRequest",
    "params": [{
      "commitment_type": 1,
      "payload": "0x000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000539",
      "slasher": "0x0000000000000000000000000000000000000000"
    }],
    "id": 1
  }'
```

### Query Slots

```bash
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"slots","params":[],"id":1}'
```

### Check Fee

```bash
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"fee","params":[1,560048],"id":1}'
```

## Mock Service Details

### Mock Beacon API (port 5051)

Simulates Ethereum Beacon Chain:
- Real-time slot calculation based on mainnet genesis
- Returns proposer duties for all slots in epoch
- BLS pubkey: `0x020202...` (48 bytes)

**Test endpoints:**
```bash
curl http://localhost:5051/eth/v1/beacon/headers/head
curl http://localhost:5051/eth/v1/validator/duties/proposer/12345
curl http://localhost:5051/health
```

### Mock Reth (port 8545)

Simulates execution client:
- Gas price: 20 gwei
- Auto-incrementing blocks every 12 seconds
- Standard JSON-RPC 2.0

**Test endpoints:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'
```

### Mock Relay (port 3501)

Simulates Constraints API:
- Returns delegations for requested slots
- Accepts constraint submissions
- Committer address: `0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266`

**Test endpoints:**
```bash
curl http://localhost:3501/constraints/v1/delegations/12345678
curl http://localhost:3501/health
```

## Monitoring

### View Logs

```bash
# Gateway logs
tail -f logs/gateway.log

# Mock service logs
tail -f logs/mock-relay.log
tail -f logs/mock-beacon-api.log
tail -f logs/mock-reth.log
```

### Check Service Status

```bash
# Check if services are running
lsof -i :3501  # Mock relay
lsof -i :5051  # Mock beacon
lsof -i :8545  # Mock reth
lsof -i :8080  # Gateway
```

## Troubleshooting

### PostgreSQL Connection Failed

```bash
# Check PostgreSQL is running
psql -c "SELECT 1" postgresql://postgres:postgres@localhost:5432/postgres

# If not, start PostgreSQL:
brew services start postgresql  # macOS
sudo systemctl start postgresql # Linux
```

### Port Already in Use

```bash
# Find and kill process using the port
lsof -i :8080
kill <PID>

# Or stop all services
./scripts/stop-mock-services.sh
```

### Gateway Won't Start

1. Check environment variables are set
2. Verify mock services are healthy
3. Check logs: `tail -f logs/gateway.log`
4. Verify database migrations: `sqlx migrate info`

### Tests Failing

1. Ensure all services are running
2. Check health endpoints
3. Review logs for errors
4. Verify database contains test data
5. Try restarting services

## Development

### Adding New Tests

Edit `tests/e2e/test-runner.ts`:

```typescript
// Add your test
console.log('Test N: My New Test');
try {
  const response = await jsonRpc(GATEWAY_URL, 'myMethod', []);
  assert(response.result !== null, 'My test assertion');
} catch (error) {
  assert(false, `My test failed: ${error}`);
}
```

### Modifying Mock Services

Each mock service is independent:

```bash
cd mock-services/beacon-api
npm run dev  # Runs with watch mode for development
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Integration Test                      │
└────────────────┬────────────────────────────────────────┘
                 │
    ┌────────────▼────────────┐
    │   Gateway (port 8080)    │
    │                          │
    │  - JSON-RPC Server       │
    │  - Delegation Polling    │
    │  - Constraint Submission │
    └─┬──────────┬───────────┬─┘
      │          │           │
      │          │           └──► PostgreSQL
      │          │                - Commitments
      │          │                - Delegations
      │          │
      ├──────────┼──────────────► Mock Beacon (5051)
      │          │                - Current slot
      │          │                - Proposer duties
      │          │
      │          └──────────────► Mock Reth (8545)
      │                           - Gas prices
      │                           - Block info
      │
      └─────────────────────────► Mock Relay (3501)
                                  - Delegations
                                  - Constraints
```

## CI/CD Integration

To run in CI/CD:

```yaml
# Example GitHub Actions
steps:
  - name: Start PostgreSQL
    run: |
      docker run -d -p 5432:5432 \
        -e POSTGRES_PASSWORD=postgres \
        postgres:15

  - name: Run Integration Tests
    run: ./scripts/integration-test.sh
    env:
      DATABASE_URL: postgresql://postgres:postgres@localhost:5432/preconfirmation_gateway
```

## Performance Testing

The mock services support high-throughput testing:

```bash
# Benchmark commitment requests
for i in {1..100}; do
  curl -s -X POST http://localhost:8080 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"commitmentRequest","params":[...],"id":'$i'}' &
done
wait
```

## Next Steps

1. ✅ Basic integration tests working
2. 🔄 Add more test scenarios (error cases, edge cases)
3. 🔄 Add performance/load testing
4. 🔄 Add Docker Compose for one-command setup
5. 🔄 Integrate with CI/CD pipeline

## Resources

- Mock Services: `./mock-services/README.md`
- Scripts: `./scripts/`
- E2E Tests: `./tests/e2e/`
- Gateway Docs: `./CLAUDE.md`, `./SYSTEM_OVERVIEW.md`
