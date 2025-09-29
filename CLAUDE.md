# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a sophisticated Rust-based preconfirmation gateway that implements the Ethereum Commitments API specification. The gateway enables validators to issue near-instant preconfirmation commitments for transaction inclusion while integrating with the Constraints API for relay communication and builder coordination.

## Development Commands

This project uses Taskfile as the task runner with `.env` file support.

### Essential Commands
- `task build` - Build the project with Cargo
- `task run` - Start the JSON-RPC server (default port 8080)
- `task test` - Run all tests (unit tests pass, some database tests ignored without DB)
- `task format` - Format code with rustfmt (hard tabs, 120 char width)
- `task lint` - Run Clippy linter

### Database Setup
The system requires PostgreSQL with migrations. Database migrations are automatically run on startup.
- Set `DATABASE_URL` environment variable or configure in `config.toml`
- Test database operations require a running PostgreSQL instance

### Testing Specific Components
- `cargo test --lib` - Run only library unit tests (avoids database dependencies)
- `cargo test test_slots_handler_service_catalog` - Run specific test
- `cargo test --package preconfirmation-gateway --lib -- crypto::tests` - Test crypto module

## High-Level Architecture

### Delegation-First Security Model
The gateway implements a **delegation-first security model** where validators delegate constraint-signing authority. All commitment requests must have valid delegation authority before any signing occurs.

### Dual Cryptographic System
- **ECDSA (secp256k1)**: Used for commitment signing (Ethereum-compatible addresses)
- **BLS (BLS12-381)**: Used for constraint signing (Ethereum 2.0 compatible)
- Both systems work together with multi-key management supporting multiple proposer delegations

### Three-Tier Request Processing
1. **Commitment Requests**: ECDSA-signed commitments with delegation verification
2. **Constraint Generation**: Background BLS-signed constraints from commitments
3. **Relay Submission**: Automatic submission to builders within 8-second windows

### Core Module Structure

```
src/
├── rpc/                   # JSON-RPC API handlers (4 methods)
│   ├── handlers.rs        # Main business logic for all endpoints
│   └── methods.rs         # Method registration and routing
├── types/                 # Type definitions and domain models
│   ├── rpc.rs            # API request/response types
│   ├── delegation.rs     # BLS delegation and constraint types
│   ├── payload.rs        # Commitment payload parsing (JSON/RLP/raw)
│   └── beacon.rs         # Ethereum beacon chain timing utilities
├── crypto/               # Cryptographic operations
│   ├── mod.rs           # ECDSA operations and hashing
│   └── bls.rs           # BLS signature operations with domain separation
├── db/                   # Database operations
│   ├── operations.rs     # Commitment storage operations
│   └── delegation_ops.rs # Delegation storage and querying
├── api/                  # External API clients
│   ├── beacon.rs        # Beacon API client for validator duties
│   └── constraints.rs   # Constraints API client for relay communication
├── services/             # Background services
│   ├── delegation_polling.rs    # Proactive delegation discovery
│   └── constraint_submission.rs # Time-critical constraint submission
└── config.rs            # Multi-layer configuration (TOML + env vars)
```

### Key Design Patterns

**Configuration Hierarchy**: TOML files + environment variables with env vars taking precedence. Private keys always loaded from environment variables for security.

**Database Abstraction**: SQLx-based with connection pooling. All operations are async with proper error handling and migrations.

**Timing-Critical Operations**: Background services handle constraint submission within strict 8-second deadlines using tokio-cron-scheduler.

**Service Catalog vs Authority**: The `slots` endpoint shows what the gateway *can* offer (service catalog), while commitment requests verify what the gateway *is delegated for* (authority validation).

## API Implementation

### Four JSON-RPC Methods
1. **commitmentRequest**: Main endpoint with delegation-first validation
2. **commitmentResult**: Query existing commitments by hash
3. **slots**: Service catalog showing available offerings (Hooli chain ID 560048 only)
4. **fee**: Fee calculation (placeholder implementation)

### Hooli Chain Integration
The system specifically supports Hooli chain (chain_id: 560048) with inclusion commitments (type 1). The slots endpoint provides a service catalog for 64 future slots (2 epochs lookahead) using real-time beacon timing calculations.

### Payload Processing
Supports multiple payload formats with robust slot extraction:
- JSON parsing (preferred)
- RLP encoding (fallback)
- Raw bytes (little-endian u64)

## Testing Architecture

### Test Organization
- **Unit Tests**: Located in each module, test individual functions
- **Integration Tests**: Found in `src/rpc/handlers.rs` test modules
- **Mock Services**: Complete mock implementations in `src/testing/`
- **Test Fixtures**: Realistic test data generators using current slot timing

### Database Test Pattern
Many tests use `DatabaseContext::new_for_testing()` to avoid actual database connections in unit tests. Real database tests are marked and skipped when `DATABASE_URL` is not available.

## Configuration Management

### Multi-Layer System
- `config.toml`: Public configuration (server, database URLs, logging)
- Environment variables: Private keys and sensitive data
- `.env` file support via Taskfile

### Security Model
- ECDSA private keys: `ECDSA_PRIVATE_KEY_1`, `ECDSA_PRIVATE_KEY_2`, etc.
- BLS private keys: `BLS_PRIVATE_KEY_1`, `BLS_PRIVATE_KEY_2`, etc.
- Private keys are NEVER stored in configuration files

## Critical Implementation Notes

### Slot Timing
The system uses 12-second Ethereum slots with beacon chain genesis time for accurate current slot calculations. All timing operations are based on `BeaconTiming::current_slot_estimate()`.

### Error Handling
Comprehensive error handling with proper JSON-RPC error codes. All database operations return detailed error context using `anyhow`.

### Performance Characteristics
- Target: 50+ TPS for commitment requests
- Actual: 29 TPS with 100% success rate and ~12ms latency
- Slots endpoint: <5ms response time (no database access)

## Documentation Maintenance

### SYSTEM_OVERVIEW.md Updates
The `SYSTEM_OVERVIEW.md` file provides comprehensive technical documentation of the system architecture and must be kept current with code changes. **Always update this document when making significant changes.**

#### When to Update SYSTEM_OVERVIEW.md
- **New API endpoints or handlers**: Add complete implementation examples with code snippets
- **New cryptographic operations**: Include detailed code examples with step-by-step explanations
- **Database schema changes**: Update SQL examples and migration information
- **New background services**: Add service lifecycle and integration details
- **Configuration changes**: Update config examples and environment variable documentation
- **Performance improvements**: Update metrics and benchmark results
- **Security model changes**: Update delegation and signing authority explanations

#### Documentation Standards
**Code Snippets**: Include complete, runnable code examples showing actual implementation:
```rust
// Example - show real function signatures and logic flow
pub async fn commitment_request_handler(
    params: jsonrpsee::types::Params<'static>,
    context: Arc<RpcContext>,
    _extensions: Extensions,
) -> RpcResult<SignedCommitment> {
    // 1. Validate commitment type (only type 1 supported)
    // 2. Extract slot from payload
    // 3. DELEGATION-FIRST SECURITY: Verify authority BEFORE signing
    // ... complete implementation flow
}
```

**Mermaid Diagrams**: Use sequence diagrams for request flows and service interactions:
- Show all participants (Client, RPC Handler, Database, Signer, etc.)
- Include both success and error paths where relevant
- Add timing-critical operations with constraints

**Architecture Sections**: Update section numbers when adding new sections:
- Core Components (numbered subsections for each major handler/service)
- Keep Request Processing Pipeline current with actual implementation
- Update Performance Characteristics with real metrics

**Response Format Examples**: Include complete JSON examples for API responses:
```json
{
  "slots": [
    {
      "slot": 12345678,
      "offerings": [
        {
          "chain_id": 560048,
          "commitment_types": [1]
        }
      ]
    }
  ]
}
```

#### Content Organization
- **Section 1**: JSON-RPC Server (commitment handlers)
- **Section 2**: Slots Service Catalog (separate from commitments)
- **Section 3**: Cryptographic Operations (ECDSA + BLS)
- **Section 4**: Payload Processing (JSON/RLP/raw formats)
- Maintain this structure when adding new sections

#### Key Documentation Principles
- **Service Catalog vs Authority**: Clearly distinguish between what the gateway "can offer" (slots) vs "is delegated for" (commitments)
- **Hooli Chain Specifics**: Always mention chain ID 560048 and inclusion commitments (type 1)
- **Security Model**: Emphasize delegation-first validation in all commitment flows
- **Real-time Calculations**: Show actual beacon timing integration, not placeholder values
- **Database-Free Operations**: Highlight which endpoints are computational-only (like slots)