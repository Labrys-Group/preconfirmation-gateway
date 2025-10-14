#!/bin/bash

# Use a specific slot that has a valid delegation (within the polled range)
SLOT=12719260

echo "Using slot: $SLOT"

# Create JSON payload
JSON_PAYLOAD="{\"slot\":$SLOT,\"signed_tx\":[1,2,3,4,5,6,7,8,9,10]}"
echo "Payload JSON: $JSON_PAYLOAD"

# Convert JSON to byte array
BYTES=$(echo -n "$JSON_PAYLOAD" | od -An -td1 | tr -d '\n' | sed 's/^ *//' | sed 's/  */,/g' | sed 's/,$//')
BYTES="[$BYTES]"

echo "Payload bytes: $BYTES"

# Make the commitment request with positional params: [commitment_type, payload, slasher]
# Using the gateway's default committer address (from default ECDSA private key)
echo -e "\nMaking commitment request..."
curl -X POST http://localhost:8080 \
  -H "Content-Type: application/json" \
  -d "{
    \"jsonrpc\": \"2.0\",
    \"method\": \"commitmentRequest\",
    \"params\": [1, $BYTES, \"0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266\"],
    \"id\": 1
  }" | jq
