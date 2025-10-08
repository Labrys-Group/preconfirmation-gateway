#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}Starting mock services for integration testing...${NC}\n"

# Ensure logs directory exists for log redirection and PID files
mkdir -p logs

# Function to check if a port is in use
port_in_use() {
  lsof -i ":$1" >/dev/null 2>&1
}

# Function to wait for service to be healthy
wait_for_service() {
  local url=$1
  local name=$2
  local max_attempts=30
  local attempt=0

  echo -n "Waiting for $name to be ready"
  while [ $attempt -lt $max_attempts ]; do
    if curl -s "$url" > /dev/null 2>&1; then
      echo -e " ${GREEN}✓${NC}"
      return 0
    fi
    echo -n "."
    sleep 1
    attempt=$((attempt + 1))
  done
  echo -e " ${YELLOW}✗${NC}"
  echo "Warning: $name did not become healthy"
  return 1
}

# Check if services are already running
if port_in_use 3501; then
  echo -e "${YELLOW}Port 3501 already in use (mock-relay)${NC}"
else
  echo -e "${BLUE}Starting mock-relay (port 3501)...${NC}"
  cd mock-relay
  npm install --silent
  npm start > ../logs/mock-relay.log 2>&1 &
  echo $! > ../logs/mock-relay.pid
  cd ..
fi

if port_in_use 5051; then
  echo -e "${YELLOW}Port 5051 already in use (mock-beacon-api)${NC}"
else
  echo -e "${BLUE}Starting mock-beacon-api (port 5051)...${NC}"
  cd mock-services/beacon-api
  npm install --silent
  npm start > ../../logs/mock-beacon-api.log 2>&1 &
  echo $! > ../../logs/mock-beacon-api.pid
  cd ../..
fi

if port_in_use 8545; then
  echo -e "${YELLOW}Port 8545 already in use (mock-reth)${NC}"
else
  echo -e "${BLUE}Starting mock-reth (port 8545)...${NC}"
  cd mock-services/reth
  npm install --silent
  npm start > ../../logs/mock-reth.log 2>&1 &
  echo $! > ../../logs/mock-reth.pid
  cd ../..
fi

echo ""

# Wait for all services to be healthy
wait_for_service "http://localhost:3501/health" "mock-relay"
wait_for_service "http://localhost:5051/health" "mock-beacon-api"
wait_for_service "http://localhost:8545/health" "mock-reth"

echo -e "\n${GREEN}All mock services are running!${NC}\n"
echo "Service URLs:"
echo "  Mock Relay:       http://localhost:3501"
echo "  Mock Beacon API:  http://localhost:5051"
echo "  Mock Reth:        http://localhost:8545"
echo ""
echo "Logs are in ./logs/"
echo ""
echo -e "To stop all services, run: ${BLUE}./scripts/stop-mock-services.sh${NC}\n"
