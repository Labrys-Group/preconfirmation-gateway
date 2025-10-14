#!/bin/bash
# Cleanup script for test databases created during integration testing
#
# Test databases are named with pattern: test_{pid}_{uuid}
# This script removes all databases matching that pattern.

set -e
set -o pipefail

DATABASE_URL="${TEST_DATABASE_URL:-postgresql://postgres:postgres@localhost:5432/postgres}"

echo "Cleaning up test databases..."

# Terminate all connections to test databases
echo "Terminating active connections to test databases..."
psql "$DATABASE_URL" -c "
  SELECT pg_terminate_backend(pid)
  FROM pg_stat_activity
  WHERE datname LIKE 'test_%' AND pid <> pg_backend_pid()
" > /dev/null

# Brief wait to ensure connections are terminated
sleep 0.5

# Get list of test databases and drop them
psql "$DATABASE_URL" -t -c "
  SELECT 'DROP DATABASE \"' || datname || '\";'
  FROM pg_database
  WHERE datname LIKE 'test_%'
" | psql "$DATABASE_URL"

# Count remaining test databases
COUNT=$(psql "$DATABASE_URL" -t -c "SELECT COUNT(*) FROM pg_database WHERE datname LIKE 'test_%'")

echo "Cleanup complete. Remaining test databases: $COUNT"
