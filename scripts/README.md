# Scripts

Utility scripts for development and testing.

## cleanup_test_dbs.sh

Removes all test databases created during integration testing.

**Usage:**
```bash
./scripts/cleanup_test_dbs.sh
```

**Environment Variables:**
- `TEST_DATABASE_URL` - PostgreSQL admin connection URL (default: `postgresql://postgres:postgres@localhost:5432/postgres`)

**What it does:**
- Connects to the PostgreSQL server
- Finds all databases matching pattern `test_%`
- Drops each test database
- Reports the count of remaining test databases

**When to use:**
- After running integration tests to clean up test databases
- If test cleanup fails automatically
- Before running tests on a clean slate
