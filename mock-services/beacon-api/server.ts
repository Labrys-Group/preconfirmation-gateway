import express from 'express';

const app = express();
const PORT = 5051;

app.use(express.json());

// Ethereum mainnet genesis time
const GENESIS_TIME = 1606824023;
const SECONDS_PER_SLOT = 12;
const SLOTS_PER_EPOCH = 32;

// Mock BLS public key (48 bytes) matching the mock-relay
const MOCK_PROPOSER_PUBKEY = '0x' + '02'.repeat(48);

// Calculate current slot based on time
function getCurrentSlot(): number {
  const now = Math.floor(Date.now() / 1000);
  const secondsSinceGenesis = now - GENESIS_TIME;
  return Math.floor(secondsSinceGenesis / SECONDS_PER_SLOT);
}

// Calculate epoch from slot
function slotToEpoch(slot: number): number {
  return Math.floor(slot / SLOTS_PER_EPOCH);
}

// Calculate first slot of epoch
function epochToFirstSlot(epoch: number): number {
  return epoch * SLOTS_PER_EPOCH;
}

/**
 * GET /eth/v1/beacon/genesis
 *
 * Returns beacon chain genesis information
 */
app.get('/eth/v1/beacon/genesis', (req, res) => {
  console.log('[Mock Beacon API] GET /eth/v1/beacon/genesis');

  res.json({
    data: {
      genesis_time: GENESIS_TIME.toString(),
      genesis_validators_root: '0x4b363db94e286120d76eb905340fdd4e54bfe9f06bf33ff6cf5ad27f511bfe95',
      genesis_fork_version: '0x00000000'
    }
  });
});

/**
 * GET /eth/v1/beacon/headers/head
 *
 * Returns the current head of the beacon chain with slot information
 */
app.get('/eth/v1/beacon/headers/head', (req, res) => {
  const currentSlot = getCurrentSlot();

  console.log(`[Mock Beacon API] GET /eth/v1/beacon/headers/head - Slot ${currentSlot}`);

  res.json({
    execution_optimistic: false,
    finalized: false,
    data: {
      root: '0x' + '01'.repeat(32),
      canonical: true,
      header: {
        message: {
          slot: currentSlot.toString(),
          proposer_index: '123',
          parent_root: '0x' + '02'.repeat(32),
          state_root: '0x' + '03'.repeat(32),
          body_root: '0x' + '04'.repeat(32)
        },
        signature: '0x' + '00'.repeat(96)
      }
    }
  });
});

/**
 * GET /eth/v1/validator/duties/proposer/:epoch
 *
 * Returns proposer duties for a specific epoch
 * For testing, we return a proposer for every slot in the epoch
 */
app.get('/eth/v1/validator/duties/proposer/:epoch', (req, res) => {
  const epoch = parseInt(req.params.epoch);

  console.log(`[Mock Beacon API] GET /eth/v1/validator/duties/proposer/${epoch}`);

  // Generate proposer duties for all slots in the epoch
  const firstSlot = epochToFirstSlot(epoch);
  const duties = [];

  for (let i = 0; i < SLOTS_PER_EPOCH; i++) {
    duties.push({
      pubkey: MOCK_PROPOSER_PUBKEY,
      validator_index: '123',
      slot: (firstSlot + i).toString()
    });
  }

  res.json({
    execution_optimistic: false,
    finalized: false,
    data: duties,
    dependent_root: '0x' + '05'.repeat(32)
  });
});

/**
 * GET /eth/v1/beacon/blocks/:block_id
 *
 * Returns a beacon block (minimal response for testing)
 */
app.get('/eth/v1/beacon/blocks/:block_id', (req, res) => {
  const blockId = req.params.block_id;

  console.log(`[Mock Beacon API] GET /eth/v1/beacon/blocks/${blockId}`);

  const currentSlot = blockId === 'head' ? getCurrentSlot() : parseInt(blockId);

  res.json({
    version: 'bellatrix',
    execution_optimistic: false,
    finalized: false,
    data: {
      message: {
        slot: currentSlot.toString(),
        proposer_index: '123',
        parent_root: '0x' + '02'.repeat(32),
        state_root: '0x' + '03'.repeat(32),
        body: {
          randao_reveal: '0x' + '00'.repeat(96),
          eth1_data: {
            deposit_root: '0x' + '06'.repeat(32),
            deposit_count: '0',
            block_hash: '0x' + '07'.repeat(32)
          },
          graffiti: '0x' + '00'.repeat(32),
          proposer_slashings: [],
          attester_slashings: [],
          attestations: [],
          deposits: [],
          voluntary_exits: []
        }
      },
      signature: '0x' + '00'.repeat(96)
    }
  });
});

/**
 * GET /eth/v1/node/health
 *
 * Health check endpoint
 */
app.get('/eth/v1/node/health', (req, res) => {
  res.status(200).send();
});

/**
 * GET /health
 *
 * Alternative health check
 */
app.get('/health', (req, res) => {
  const currentSlot = getCurrentSlot();
  const currentEpoch = slotToEpoch(currentSlot);

  res.json({
    status: 'healthy',
    service: 'mock-beacon-api',
    timestamp: new Date().toISOString(),
    current_slot: currentSlot,
    current_epoch: currentEpoch,
    genesis_time: GENESIS_TIME
  });
});

app.listen(PORT, () => {
  const currentSlot = getCurrentSlot();
  const currentEpoch = slotToEpoch(currentSlot);

  console.log(`\n🔮 Mock Beacon API running on http://localhost:${PORT}`);
  console.log(`\nCurrent State:`);
  console.log(`  Slot:  ${currentSlot}`);
  console.log(`  Epoch: ${currentEpoch}`);
  console.log(`\nEndpoints:`);
  console.log(`  GET  /eth/v1/beacon/genesis - Beacon chain genesis`);
  console.log(`  GET  /eth/v1/beacon/headers/head - Current head slot`);
  console.log(`  GET  /eth/v1/validator/duties/proposer/:epoch - Proposer duties`);
  console.log(`  GET  /eth/v1/beacon/blocks/:block_id - Beacon blocks`);
  console.log(`  GET  /eth/v1/node/health - Health check`);
  console.log(`  GET  /health - Health status with slot info\n`);
});
