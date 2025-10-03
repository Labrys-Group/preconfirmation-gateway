# Mock Constraints API Relay

A simple mock relay server for testing the preconfirmation gateway.

## What it does

Implements the Constraints API endpoints that the gateway expects:
- `GET /constraints/v1/delegations/:slot` - Returns mock delegation data
- `POST /constraints/v1/builder/constraints` - Accepts constraint submissions

## Setup

```bash
cd mock-relay
npm install
```

## Run

```bash
npm start
```

The server runs on port 3501 (configured in the gateway's `config.toml`).

## Testing

The gateway will automatically poll this relay every 30 seconds for delegations.
You can also test the endpoints directly:

```bash
# Get delegations for a slot
curl http://localhost:3501/constraints/v1/delegations/12713612

# Health check
curl http://localhost:3501/health
```
