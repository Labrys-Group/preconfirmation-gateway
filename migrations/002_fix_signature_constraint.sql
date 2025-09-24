-- Fix signature format constraint
-- ECDSA signatures are 64 bytes = 128 hex characters, not 130

-- Update the signature column constraint
ALTER TABLE commitments DROP CONSTRAINT chk_signature_format;
ALTER TABLE commitments ADD CONSTRAINT chk_signature_format CHECK (signature ~ '^0x[a-fA-F0-9]{128}$');

-- Update the column size as well
ALTER TABLE commitments ALTER COLUMN signature TYPE VARCHAR(130);

-- Update comment for accuracy
COMMENT ON COLUMN commitments.signature IS 'ECDSA signature over keccak256(abi.encode(commitment)) - 64 bytes as hex (0x + 128 chars)';