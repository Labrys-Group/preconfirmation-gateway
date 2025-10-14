import express from 'express';

const app = express();
const PORT = 3501;

app.use(express.json());

// Mock BLS public keys (48 bytes each)
const MOCK_DELEGATEE_PUBKEY = '0x' + '01'.repeat(48);  // Gateway's BLS key
const MOCK_PROPOSER_PUBKEY = '0x' + '02'.repeat(48);   // Validator's BLS key

// Mock committer address (Ethereum address used by gateway for ECDSA signing)
// This matches the gateway's default ECDSA address derived from the default private key
const MOCK_COMMITTER_ADDRESS = '0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266';

// SignedDelegation structure matching the gateway's types
interface DelegationMessage {
  proposer: string;   // BLS public key (48 bytes)
  delegate: string;   // BLS public key (48 bytes)
  committer: string;  // Ethereum address
  slot: number;
}

interface SignedDelegation {
  message: DelegationMessage;
  signature: string;  // BLS signature (96 bytes)
}

/**
 * GET /constraints/v1/delegations/{slot}
 *
 * Returns delegations for a specific slot.
 * The gateway polls this endpoint every 30 seconds for upcoming slots.
 */
app.get('/constraints/v1/delegations/:slot', (req, res) => {
  const slot = parseInt(req.params.slot);

  console.log(`[Mock Relay] GET /constraints/v1/delegations/${slot}`);

  // Return a delegation for the requested slot
  const delegations: SignedDelegation[] = [
    {
      message: {
        proposer: MOCK_PROPOSER_PUBKEY,
        delegate: MOCK_DELEGATEE_PUBKEY,
        committer: MOCK_COMMITTER_ADDRESS,
        slot: slot,
      },
      signature: '0x' + '00'.repeat(96), // Mock BLS signature (96 bytes)
    }
  ];

  console.log(`[Mock Relay] Returning ${delegations.length} delegation(s)`);
  res.json({ delegations });
});

/**
 * POST /constraints/v1/builder/constraints
 *
 * Accepts constraint submissions from the gateway.
 * The gateway submits constraints in the background for included transactions.
 */
app.post('/constraints/v1/builder/constraints', (req, res) => {
  console.log('[Mock Relay] POST /constraints/v1/builder/constraints');
  console.log('[Mock Relay] Received constraints:', JSON.stringify(req.body, null, 2));

  // Accept the constraint
  res.status(200).json({
    message: 'Constraint accepted',
    received_at: new Date().toISOString()
  });
});

/**
 * Health check endpoint
 */
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'mock-constraints-relay',
    timestamp: new Date().toISOString()
  });
});

app.listen(PORT, () => {
  console.log(`\n🚀 Mock Constraints API Relay running on http://localhost:${PORT}`);
  console.log(`\nEndpoints:`);
  console.log(`  GET  /constraints/v1/delegations/:slot - Returns mock delegations`);
  console.log(`  POST /constraints/v1/builder/constraints - Accepts constraints`);
  console.log(`  GET  /health - Health check`);
  console.log(`\nThe gateway will poll delegations every 30 seconds.\n`);
});
