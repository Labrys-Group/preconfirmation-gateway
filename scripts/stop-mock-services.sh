#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}Stopping mock services...${NC}\n"

# Function to stop a service by PID file
stop_service() {
  local pid_file=$1
  local name=$2

  if [ -f "$pid_file" ]; then
    local pid=$(cat "$pid_file")
    if kill -0 "$pid" 2>/dev/null; then
      echo -e "Stopping $name (PID $pid)..."
      kill "$pid" 2>/dev/null || true
      rm "$pid_file"
      echo -e "  ${GREEN}✓${NC} Stopped"
    else
      echo -e "$name PID file exists but process not running"
      rm "$pid_file"
    fi
  else
    echo -e "$name not running (no PID file)"
  fi
}

# Create logs directory if it doesn't exist
mkdir -p logs

# Stop all services
stop_service "logs/mock-relay.pid" "mock-relay"
stop_service "logs/mock-beacon-api.pid" "mock-beacon-api"
stop_service "logs/mock-reth.pid" "mock-reth"
stop_service "logs/gateway.pid" "gateway"

echo -e "\n${GREEN}All services stopped${NC}\n"
