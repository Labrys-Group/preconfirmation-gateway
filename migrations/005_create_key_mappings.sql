-- Create key mappings table for managing Gateway's multiple keys
-- Maps delegation addresses to Gateway's private key identifiers

CREATE TABLE gateway_key_mappings (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Key identities
    committer_address VARCHAR(42) NOT NULL,  -- ECDSA address for commitment signing
    delegate_pubkey BYTEA NOT NULL,          -- BLS public key for constraint signing (48 bytes)

    -- Key references (for secure key management)
    ecdsa_key_id VARCHAR(255) NOT NULL,      -- Identifier for ECDSA private key (e.g., env var name)
    bls_key_id VARCHAR(255) NOT NULL,        -- Identifier for BLS private key (e.g., env var name)

    -- Key metadata
    key_name VARCHAR(100),                   -- Human-readable name for this key pair
    is_active BOOLEAN DEFAULT TRUE,          -- Whether this key pair is currently active
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_committer_format CHECK (committer_address ~ '^0x[a-fA-F0-9]{40}$'),
    CONSTRAINT chk_delegate_pubkey_length CHECK (LENGTH(delegate_pubkey) = 48),
    CONSTRAINT chk_ecdsa_key_id_not_empty CHECK (LENGTH(TRIM(ecdsa_key_id)) > 0),
    CONSTRAINT chk_bls_key_id_not_empty CHECK (LENGTH(TRIM(bls_key_id)) > 0),

    -- Unique constraints
    CONSTRAINT unique_committer_address UNIQUE (committer_address),
    CONSTRAINT unique_delegate_pubkey UNIQUE (delegate_pubkey),
    CONSTRAINT unique_ecdsa_key_id UNIQUE (ecdsa_key_id),
    CONSTRAINT unique_bls_key_id UNIQUE (bls_key_id)
);

-- Indexes for efficient querying
CREATE INDEX idx_key_mappings_committer ON gateway_key_mappings(committer_address);
CREATE INDEX idx_key_mappings_delegate ON gateway_key_mappings(delegate_pubkey);
CREATE INDEX idx_key_mappings_active ON gateway_key_mappings(is_active) WHERE is_active = true;

-- Create a function to update the updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Trigger to automatically update updated_at
CREATE TRIGGER update_key_mappings_updated_at
    BEFORE UPDATE ON gateway_key_mappings
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Comments for documentation
COMMENT ON TABLE gateway_key_mappings IS 'Maps delegation addresses to Gateway private key identifiers';
COMMENT ON COLUMN gateway_key_mappings.committer_address IS 'ECDSA address used in delegations for commitment signing';
COMMENT ON COLUMN gateway_key_mappings.delegate_pubkey IS 'BLS public key used in delegations for constraint signing';
COMMENT ON COLUMN gateway_key_mappings.ecdsa_key_id IS 'Environment variable or key store identifier for ECDSA private key';
COMMENT ON COLUMN gateway_key_mappings.bls_key_id IS 'Environment variable or key store identifier for BLS private key';
COMMENT ON COLUMN gateway_key_mappings.key_name IS 'Human-readable identifier for operational management';
COMMENT ON COLUMN gateway_key_mappings.is_active IS 'Whether this key pair is currently accepting new delegations';