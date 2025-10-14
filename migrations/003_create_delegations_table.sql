-- Create delegations table for storing SignedDelegation messages
-- This table stores delegation authority from proposers to the Gateway

CREATE TABLE delegations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Delegation message fields
    proposer_pubkey BYTEA NOT NULL,     -- BLS public key of the proposer (48 bytes)
    delegate_pubkey BYTEA NOT NULL,     -- BLS public key of the Gateway (48 bytes)
    committer_address VARCHAR(42) NOT NULL, -- ECDSA address for commitment signing
    slot_number BIGINT NOT NULL,        -- Specific slot this delegation applies to

    -- BLS signature over the delegation message
    signature BYTEA NOT NULL,           -- BLS signature (96 bytes)

    -- Additional metadata
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ,            -- Optional expiration time
    is_active BOOLEAN DEFAULT TRUE,     -- Whether this delegation is currently active

    -- Constraints
    CONSTRAINT chk_proposer_pubkey_length CHECK (LENGTH(proposer_pubkey) = 48),
    CONSTRAINT chk_delegate_pubkey_length CHECK (LENGTH(delegate_pubkey) = 48),
    CONSTRAINT chk_signature_length CHECK (LENGTH(signature) = 96),
    CONSTRAINT chk_committer_format CHECK (committer_address ~ '^0x[a-fA-F0-9]{40}$'),
    CONSTRAINT chk_slot_positive CHECK (slot_number >= 0),

    -- Unique constraint to prevent duplicate delegations
    CONSTRAINT unique_delegation_per_slot UNIQUE (proposer_pubkey, slot_number)
);

-- Indexes for efficient querying
CREATE INDEX idx_delegations_slot ON delegations(slot_number);
CREATE INDEX idx_delegations_delegate_pubkey ON delegations(delegate_pubkey);
CREATE INDEX idx_delegations_committer_address ON delegations(committer_address);
CREATE INDEX idx_delegations_active_slot ON delegations(slot_number, is_active) WHERE is_active = true;
CREATE INDEX idx_delegations_expires_at ON delegations(expires_at) WHERE expires_at IS NOT NULL;

-- Comments for documentation
COMMENT ON TABLE delegations IS 'Stores SignedDelegation messages granting Gateway authority for specific slots';
COMMENT ON COLUMN delegations.proposer_pubkey IS 'BLS public key of validator delegating authority (48 bytes)';
COMMENT ON COLUMN delegations.delegate_pubkey IS 'BLS public key of Gateway receiving authority (48 bytes)';
COMMENT ON COLUMN delegations.committer_address IS 'ECDSA address Gateway must use for commitment signing';
COMMENT ON COLUMN delegations.slot_number IS 'Specific beacon chain slot this delegation applies to';
COMMENT ON COLUMN delegations.signature IS 'BLS signature by proposer over delegation message (96 bytes)';
COMMENT ON COLUMN delegations.is_active IS 'Whether delegation is currently valid and active';