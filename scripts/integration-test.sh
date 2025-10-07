#!/bin/bash
set -euo pipefail

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Preconfirmation Gateway Integration Test Suite       ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════╝${NC}\n"

# Create logs directory
mkdir -p logs

# Cleanup function
cleanup() {
  echo -e "\n${BLUE}Cleaning up...${NC}"
  ./scripts/stop-mock-services.sh
  if [ -f "logs/gateway.pid" ]; then
    kill $(cat logs/gateway.pid) 2>/dev/null || true
    rm logs/gateway.pid
  fi
}

trap cleanup EXIT

# Step 1: Check PostgreSQL
echo -e "${BLUE}[1/8]${NC} Checking PostgreSQL..."
if ! psql -c "SELECT 1" postgresql://postgres:postgres@localhost:5432/postgres > /dev/null 2>&1; then
  echo -e "${RED}✗ PostgreSQL is not running or not accessible${NC}"
  echo "Please start PostgreSQL and ensure it's accessible at: postgresql://postgres:postgres@localhost:5432/postgres"
  exit 1
fi
echo -e "${GREEN}✓ PostgreSQL is running${NC}\n"

# Step 2: Start mock services
echo -e "${BLUE}[2/8]${NC} Starting mock services..."
./scripts/start-mock-services.sh
echo ""

# Step 3: Setup test environment
echo -e "${BLUE}[3/8]${NC} Setting up test environment..."

# Export environment variables for the gateway
export DATABASE_URL="postgresql://postgres:postgres@localhost:5432/preconfirmation_gateway"
export BEACON_API_ENDPOINT="http://localhost:5051"
export RETH_ENDPOINT="http://localhost:8545"
export CONSTRAINTS_API_ENDPOINT="http://localhost:3501"

# Generate test keys if not set
if [ -z "${ECDSA_PRIVATE_KEY_1:-}" ]; then
  # Default test key (matches mock-relay committer address)
  export ECDSA_PRIVATE_KEY_1="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
fi

if [ -z "${BLS_PRIVATE_KEY_1:-}" ]; then
  # Mock BLS key (for testing)
  export BLS_PRIVATE_KEY_1="0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001"
fi

echo -e "${GREEN}✓ Environment configured${NC}\n"

# Step 4: Run database migrations
echo -e "${BLUE}[4/8]${NC} Running database migrations..."
sqlx migrate run > logs/migrations.log 2>&1
echo -e "${GREEN}✓ Migrations complete${NC}\n"

# Step 5: Build the gateway
echo -e "${BLUE}[5/8]${NC} Building gateway..."
cargo build --release > logs/build.log 2>&1
echo -e "${GREEN}✓ Gateway built${NC}\n"

# Step 6: Start the gateway
echo -e "${BLUE}[6/8]${NC} Starting gateway..."
./target/release/preconfirmation-gateway > logs/gateway.log 2>&1 &
GATEWAY_PID=$!
echo $GATEWAY_PID > logs/gateway.pid

# Wait for gateway to be ready
echo -n "Waiting for gateway"
MAX_ATTEMPTS=30
ATTEMPT=0
while [ $ATTEMPT -lt $MAX_ATTEMPTS ]; do
  if curl -s -X POST http://localhost:8080 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"slots","params":[],"id":1}' > /dev/null 2>&1; then
    echo -e " ${GREEN}✓${NC}"
    break
  fi
  echo -n "."
  sleep 1
  ATTEMPT=$((ATTEMPT + 1))
done

if [ $ATTEMPT -eq $MAX_ATTEMPTS ]; then
  echo -e " ${RED}✗${NC}"
  echo "Gateway failed to start. Check logs/gateway.log"
  exit 1
fi
echo ""

# Step 7: Run integration tests
echo -e "${BLUE}[7/8]${NC} Running integration tests..."
echo ""

# Test 1: Slots endpoint
echo -n "  Testing slots endpoint... "
SLOTS_RESPONSE=$(curl -s -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"slots","params":[],"id":1}')

if echo "$SLOTS_RESPONSE" | jq -e '.result.slots' > /dev/null 2>&1; then
  SLOT_COUNT=$(echo "$SLOTS_RESPONSE" | jq '.result.slots | length')
  echo -e "${GREEN}✓${NC} (returned $SLOT_COUNT slots)"
else
  echo -e "${RED}✗${NC}"
  echo "Response: $SLOTS_RESPONSE"
fi

# Test 2: Fee endpoint
echo -n "  Testing fee endpoint... "
FEE_RESPONSE=$(curl -s -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"fee","params":[1,560048],"id":1}')

if echo "$FEE_RESPONSE" | jq -e '.result' > /dev/null 2>&1; then
  echo -e "${GREEN}✓${NC}"
else
  echo -e "${RED}✗${NC}"
  echo "Response: $FEE_RESPONSE"
fi

# Test 3: Commitment request
echo -n "  Testing commitment request... "

# Create a test payload (ABI-encoded inclusion payload)
# This is a simplified version - in production you'd use proper ABI encoding
TEST_PAYLOAD="0x000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000539"

COMMITMENT_RESPONSE=$(curl -s -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"commitmentRequest\",\"params\":[{\"commitment_type\":1,\"payload\":\"$TEST_PAYLOAD\",\"slasher\":\"0x0000000000000000000000000000000000000000\"}],\"id\":1}")

if echo "$COMMITMENT_RESPONSE" | jq -e '.result.signature' > /dev/null 2>&1; then
  REQUEST_HASH=$(echo "$COMMITMENT_RESPONSE" | jq -r '.result.commitment.request_hash')
  echo -e "${GREEN}✓${NC} (hash: ${REQUEST_HASH:0:18}...)"

  # Test 4: Commitment result (retrieval)
  echo -n "  Testing commitment result... "
  RESULT_RESPONSE=$(curl -s -X POST http://localhost:8080 \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"commitmentResult\",\"params\":[\"$REQUEST_HASH\"],\"id\":1}")

  if echo "$RESULT_RESPONSE" | jq -e '.result.signature' > /dev/null 2>&1; then
    echo -e "${GREEN}✓${NC}"
  else
    echo -e "${RED}✗${NC}"
    echo "Response: $RESULT_RESPONSE"
  fi
else
  echo -e "${RED}✗${NC}"
  echo "Response: $COMMITMENT_RESPONSE"
fi

echo ""

# Step 8: Verify background services
echo -e "${BLUE}[8/8]${NC} Checking background services..."
echo -n "  Waiting for delegation polling... "
sleep 15 # Wait for at least one poll cycle
if grep -q "delegation" logs/gateway.log 2>/dev/null; then
  echo -e "${GREEN}✓${NC}"
else
  echo -e "${YELLOW}~ (check logs/gateway.log)${NC}"
fi

echo -n "  Checking constraint submission service... "
if grep -q "constraint" logs/gateway.log 2>/dev/null; then
  echo -e "${GREEN}✓${NC}"
else
  echo -e "${YELLOW}~ (check logs/gateway.log)${NC}"
fi

echo ""

# Summary
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}✓ Integration tests complete!${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}\n"

echo "Next steps:"
echo "  - Check logs in ./logs/ for detailed output"
echo "  - Monitor services with: tail -f logs/gateway.log"
echo "  - Stop services with: ./scripts/stop-mock-services.sh"
echo ""
