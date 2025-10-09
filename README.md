# Preconfirmation Gateway

A Rust-based preconfirmation gateway that enables Ethereum validators to issue commitments for transaction inclusion. The gateway implements the Commitments API specification and integrates with the Constraints API for relay communication, providing near-instant preconfirmation responses to users while ensuring compliance with Ethereum's block construction requirements.

## Getting Started

### Prerequisites

- Rust (latest stable version)
- Cargo
- Taskfile
- PostgreSQL
- [sqlx-cli](https://github.com/launchbadge/sqlx) (for database migrations)

### Database Setup

Install sqlx-cli:
```bash
cargo install sqlx-cli --no-default-features --features postgres
```

Set up your database URL in `.env`:
```bash
DATABASE_URL=postgresql://user:password@localhost/preconf_gateway
```

Run migrations:
```bash
sqlx database create
sqlx migrate run
```

### Building

```bash
task install
task build
```

### Running

```bash
task run
```

This starts the RPC server on `127.0.0.1` with a random port and demonstrates client connectivity.

### Development

#### Code Formatting
```bash
task format
```

#### Linting
```bash
task lint
```

#### Testing
```bash
task test
```

## Architecture

The preconfirmation gateway implements a comprehensive system for Ethereum transaction preconfirmations:

### Core Features
- **Commitments API**: Implements 4 JSON-RPC methods (`commitmentRequest`, `commitmentResult`, `slots`, `fee`)
- **Validator Integration**: Issues commitments on behalf of Ethereum proposers
- **Constraint Management**: Creates and disseminates constraints to builders via relay integration
- **Gas Pricing**: Dynamic pricing using rETH gas oracle with configurable pricing curves
- **First-Come-First-Serve**: Ensures fair commitment dispensing with near-instant response times

### Technical Implementation
- **PostgreSQL Integration**: Persistent storage for commitments, delegations, and slot metadata
- **BLS/ECDSA Cryptography**: Secure signature verification and commitment signing
- **Slot Timing Management**: 8-second constraint submission windows with automated scheduling
- **Relay Communication**: Integration with Constraints API for builder coordination

## Dependencies

### Core Runtime
- **jsonrpsee**: JSON-RPC 2.0 server implementation
- **tokio**: Async runtime with multi-threading support
- **deadpool-postgres**: Async PostgreSQL connection pooling
- **tracing**: Structured logging and diagnostics

## Specifications

This project implements the following Ethereum preconfirmation specifications:
- [Commitments API](https://github.com/eth-fabric/commitments-specs/blob/main/specs/commitments-api.md)
- [Constraints API](https://github.com/eth-fabric/constraints-specs/blob/main/specs/constraints-api.md)
- [Gateway Specification](https://github.com/eth-fabric/constraints-specs/blob/main/specs/gateway.md)