-- Fix signature format constraint
-- ECDSA signatures MUST be recoverable format (65 bytes = 130 hex characters)
-- This includes the recovery ID (v parameter) required for standard Ethereum signature verification

-- Update the signature column constraint to match recoverable signature format
ALTER TABLE commitments DROP CONSTRAINT chk_signature_format;
ALTER TABLE commitments ADD CONSTRAINT chk_signature_format CHECK (signature ~ '^0x[a-fA-F0-9]{130}$');

-- Column size is already VARCHAR(132) from migration 001 - no change needed

-- Update comment for accuracy
COMMENT ON COLUMN commitments.signature IS 'ECDSA recoverable signature over keccak256(abi.encode(commitment)) - 65 bytes as hex (0x + 130 chars: r||s||v)';