#!/usr/bin/env tsx

/**
 * End-to-End Test Runner for Preconfirmation Gateway
 *
 * This test suite validates the complete integration of all gateway components
 * with mock services simulating the external dependencies.
 */

import { createHash } from 'crypto';

// Configuration
const GATEWAY_URL = 'http://localhost:8080';
const MOCK_RELAY_URL = 'http://localhost:3501';
const MOCK_BEACON_URL = 'http://localhost:5051';
const MOCK_RETH_URL = 'http://localhost:8545';

// Test results tracking
let passed = 0;
let failed = 0;
const failures: string[] = [];

// Helper function to make JSON-RPC calls
async function jsonRpc(url: string, method: string, params: any[] = [], id: number = 1) {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      method,
      params,
      id
    })
  });

  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  }

  return await response.json();
}

// Test assertion helper
function assert(condition: boolean, message: string) {
  if (condition) {
    passed++;
    console.log(`  ✓ ${message}`);
  } else {
    failed++;
    failures.push(message);
    console.log(`  ✗ ${message}`);
  }
}

// Test helpers
async function testServiceHealth(url: string, name: string) {
  try {
    const response = await fetch(`${url}/health`);
    assert(response.ok, `${name} is healthy`);
  } catch (error) {
    assert(false, `${name} is healthy (${error})`);
  }
}

// Main test suite
async function runTests() {
  console.log('\n═══════════════════════════════════════════════════════');
  console.log('  E2E Test Suite - Preconfirmation Gateway');
  console.log('═══════════════════════════════════════════════════════\n');

  // Test 1: Service Health Checks
  console.log('Test 1: Service Health Checks');
  await testServiceHealth(MOCK_RELAY_URL, 'Mock Relay');
  await testServiceHealth(MOCK_BEACON_URL, 'Mock Beacon API');
  await testServiceHealth(MOCK_RETH_URL, 'Mock Reth');
  console.log('');

  // Test 2: Slots Endpoint (Service Catalog)
  console.log('Test 2: Slots Endpoint - Service Catalog');
  try {
    const slotsResponse = await jsonRpc(GATEWAY_URL, 'slots', []);

    assert('result' in slotsResponse && !('error' in slotsResponse), 'Slots request succeeds');
    assert(Array.isArray(slotsResponse.result?.slots), 'Returns slots array');
    assert(slotsResponse.result.slots.length > 0, 'Returns non-empty slots');
    assert(slotsResponse.result.slots.length === 64, 'Returns 64 slots (2 epochs)');

    // Check first slot structure
    const firstSlot = slotsResponse.result.slots[0];
    assert(typeof firstSlot.slot === 'number', 'Slot has numeric slot number');
    assert(Array.isArray(firstSlot.offerings), 'Slot has offerings array');

    // Check Hooli chain offering
    const hooliOffering = firstSlot.offerings.find((o: any) => o.chain_id === 560048);
    assert(!!hooliOffering, 'Includes Hooli chain (560048)');
    assert(hooliOffering?.commitment_types?.includes(1), 'Supports inclusion commitment (type 1)');
  } catch (error) {
    assert(false, `Slots endpoint works (${error})`);
  }
  console.log('');

  // Test 3: Fee Endpoint
  console.log('Test 3: Fee Endpoint - Dynamic Pricing');
  try {
    const feeResponse = await jsonRpc(GATEWAY_URL, 'fee', [1, 560048]);

    assert('result' in feeResponse && !('error' in feeResponse), 'Fee request succeeds');
    assert(typeof feeResponse.result === 'string' || typeof feeResponse.result === 'number', 'Returns fee value');
  } catch (error) {
    assert(false, `Fee endpoint works (${error})`);
  }
  console.log('');

  // Test 4: Commitment Request
  console.log('Test 4: Commitment Request - Full Flow');

  // Create a valid test payload (simplified ABI encoding)
  const testSlot = Math.floor(Date.now() / 1000 / 12);
  const testPayload = '0x' + Buffer.concat([
    Buffer.from('0000000000000000000000000000000000000000000000000000000000000020', 'hex'),
    Buffer.from(testSlot.toString(16).padStart(16, '0'), 'hex')
  ]).toString('hex');

  try {
    const commitmentResponse = await jsonRpc(GATEWAY_URL, 'commitmentRequest', [{
      commitment_type: 1,
      payload: testPayload,
      slasher: '0x0000000000000000000000000000000000000000'
    }]);

    assert('result' in commitmentResponse && !('error' in commitmentResponse), 'Commitment request succeeds');

    const result = commitmentResponse.result;
    assert(result.commitment?.commitment_type === 1, 'Commitment type is 1');
    assert(result.commitment?.request_hash?.startsWith('0x'), 'Has request hash');
    assert(result.commitment?.request_hash?.length === 66, 'Request hash is 66 chars (0x + 64)');
    assert(result.signature?.startsWith('0x'), 'Has ECDSA signature');
    assert(result.signature?.length === 132, 'Signature is 132 chars (0x + 130)');

    const requestHash = result.commitment.request_hash;
    console.log(`  → Request hash: ${requestHash.substring(0, 18)}...`);

    // Test 5: Commitment Result (Retrieval)
    console.log('');
    console.log('Test 5: Commitment Result - Retrieval');

    const resultResponse = await jsonRpc(GATEWAY_URL, 'commitmentResult', [requestHash]);

    assert('result' in resultResponse && !('error' in resultResponse), 'Commitment result succeeds');
    assert(resultResponse.result?.commitment?.request_hash === requestHash, 'Retrieved commitment matches');
    assert(resultResponse.result?.signature === result.signature, 'Signature matches original');

  } catch (error) {
    assert(false, `Commitment flow works (${error})`);
  }
  console.log('');

  // Test 6: Mock Relay Integration
  console.log('Test 6: Mock Relay Integration');
  try {
    // Get current slot
    const beaconResponse = await fetch(`${MOCK_BEACON_URL}/eth/v1/beacon/headers/head`);
    const beaconData = await beaconResponse.json();
    const currentSlot = parseInt(beaconData.data.header.message.slot);

    // Query delegations from mock relay
    const delegationsUrl = `${MOCK_RELAY_URL}/constraints/v1/delegations/${currentSlot}`;
    const delegationsResponse = await fetch(delegationsUrl);
    const delegationsData = await delegationsResponse.json();

    assert(Array.isArray(delegationsData.delegations), 'Mock relay returns delegations');
    assert(delegationsData.delegations.length > 0, 'Delegations array is non-empty');
  } catch (error) {
    assert(false, `Mock relay integration works (${error})`);
  }
  console.log('');

  // Test 7: Error Handling
  console.log('Test 7: Error Handling');
  try {
    // Test invalid commitment type
    const invalidResponse = await jsonRpc(GATEWAY_URL, 'commitmentRequest', [{
      commitment_type: 999, // Invalid type
      payload: '0x1234',
      slasher: '0x0000000000000000000000000000000000000000'
    }]);

    assert('error' in invalidResponse, 'Rejects invalid commitment type');
  } catch (error) {
    assert(false, `Error handling works (${error})`);
  }

  try {
    // Test non-existent commitment retrieval
    const nonExistentHash = '0x' + '00'.repeat(32);
    const notFoundResponse = await jsonRpc(GATEWAY_URL, 'commitmentResult', [nonExistentHash]);

    assert('result' in notFoundResponse, 'Returns result for non-existent commitment');
    assert(notFoundResponse.result === null || notFoundResponse.result === undefined, 'Result is null for non-existent');
  } catch (error) {
    assert(false, `Non-existent commitment handling works (${error})`);
  }
  console.log('');

  // Results Summary
  console.log('═══════════════════════════════════════════════════════');
  console.log(`\nTest Results: ${passed} passed, ${failed} failed\n`);

  if (failed > 0) {
    console.log('Failed tests:');
    failures.forEach(f => console.log(`  - ${f}`));
    console.log('');
    process.exit(1);
  } else {
    console.log('✓ All tests passed!\n');
    process.exit(0);
  }
}

// Run tests
runTests().catch(error => {
  console.error('Test suite failed:', error);
  process.exit(1);
});
