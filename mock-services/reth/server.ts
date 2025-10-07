import express from 'express';

const app = express();
const PORT = 8545;

app.use(express.json());

// Mock gas price (20 gwei in wei)
const MOCK_GAS_PRICE = '0x4a817c800'; // 20000000000 wei = 20 gwei

// Mock block number
let currentBlockNumber = 19000000;

// Increment block number every 12 seconds
setInterval(() => {
  currentBlockNumber++;
}, 12000);

/**
 * JSON-RPC 2.0 request handler
 */
interface JsonRpcRequest {
  jsonrpc: string;
  method: string;
  params?: any[];
  id: number | string;
}

interface JsonRpcResponse {
  jsonrpc: string;
  result?: any;
  error?: {
    code: number;
    message: string;
  };
  id: number | string;
}

/**
 * Handle JSON-RPC requests
 */
function handleJsonRpcRequest(req: JsonRpcRequest): JsonRpcResponse {
  const { method, params, id } = req;

  console.log(`[Mock Reth] RPC Method: ${method}${params ? ` Params: ${JSON.stringify(params)}` : ''}`);

  switch (method) {
    case 'eth_gasPrice':
      return {
        jsonrpc: '2.0',
        result: MOCK_GAS_PRICE,
        id
      };

    case 'eth_blockNumber':
      return {
        jsonrpc: '2.0',
        result: '0x' + currentBlockNumber.toString(16),
        id
      };

    case 'eth_getBlockByNumber': {
      const [blockNumber, fullTx] = params || ['latest', false];
      return {
        jsonrpc: '2.0',
        result: {
          number: '0x' + currentBlockNumber.toString(16),
          hash: '0x' + '01'.repeat(32),
          parentHash: '0x' + '02'.repeat(32),
          nonce: '0x0000000000000000',
          sha3Uncles: '0x' + '03'.repeat(32),
          logsBloom: '0x' + '00'.repeat(256),
          transactionsRoot: '0x' + '04'.repeat(32),
          stateRoot: '0x' + '05'.repeat(32),
          receiptsRoot: '0x' + '06'.repeat(32),
          miner: '0x' + '07'.repeat(20),
          difficulty: '0x0',
          totalDifficulty: '0x0',
          extraData: '0x',
          size: '0x' + (1000).toString(16),
          gasLimit: '0x' + (30000000).toString(16),
          gasUsed: '0x' + (15000000).toString(16),
          timestamp: '0x' + Math.floor(Date.now() / 1000).toString(16),
          transactions: fullTx ? [] : [],
          uncles: [],
          baseFeePerGas: '0x' + (10 * 1e9).toString(16) // 10 gwei
        },
        id
      };
    }

    case 'eth_chainId':
      return {
        jsonrpc: '2.0',
        result: '0x1', // Ethereum mainnet
        id
      };

    case 'net_version':
      return {
        jsonrpc: '2.0',
        result: '1', // Ethereum mainnet
        id
      };

    case 'eth_syncing':
      return {
        jsonrpc: '2.0',
        result: false, // Not syncing
        id
      };

    case 'web3_clientVersion':
      return {
        jsonrpc: '2.0',
        result: 'MockReth/v1.0.0/mock',
        id
      };

    default:
      return {
        jsonrpc: '2.0',
        error: {
          code: -32601,
          message: `Method ${method} not supported by mock`
        },
        id
      };
  }
}

/**
 * POST / - JSON-RPC endpoint
 *
 * Accepts standard JSON-RPC 2.0 requests
 */
app.post('/', (req, res) => {
  const request = req.body as JsonRpcRequest;

  // Validate JSON-RPC format
  if (!request.jsonrpc || request.jsonrpc !== '2.0' || !request.method) {
    res.status(400).json({
      jsonrpc: '2.0',
      error: {
        code: -32600,
        message: 'Invalid Request'
      },
      id: request.id || null
    });
    return;
  }

  const response = handleJsonRpcRequest(request);
  res.json(response);
});

/**
 * GET /health
 *
 * Health check endpoint
 */
app.get('/health', (req, res) => {
  res.json({
    status: 'healthy',
    service: 'mock-reth',
    timestamp: new Date().toISOString(),
    current_block: currentBlockNumber,
    gas_price: MOCK_GAS_PRICE + ' (' + (parseInt(MOCK_GAS_PRICE, 16) / 1e9) + ' gwei)'
  });
});

app.listen(PORT, () => {
  console.log(`\n⛓️  Mock Reth (Execution Client) running on http://localhost:${PORT}`);
  console.log(`\nSupported JSON-RPC Methods:`);
  console.log(`  eth_gasPrice - Returns current gas price`);
  console.log(`  eth_blockNumber - Returns current block number`);
  console.log(`  eth_getBlockByNumber - Returns block information`);
  console.log(`  eth_chainId - Returns chain ID`);
  console.log(`  net_version - Returns network version`);
  console.log(`  eth_syncing - Returns sync status`);
  console.log(`  web3_clientVersion - Returns client version`);
  console.log(`\nAdditional Endpoints:`);
  console.log(`  GET  /health - Health check with current state`);
  console.log(`\nCurrent State:`);
  console.log(`  Block Number: ${currentBlockNumber}`);
  console.log(`  Gas Price: ${parseInt(MOCK_GAS_PRICE, 16) / 1e9} gwei\n`);
});
