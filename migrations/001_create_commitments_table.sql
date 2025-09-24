-- Create commitments table for storing signed commitments
-- This table stores the core commitment data according to the Gateway specification

CREATE TABLE commitments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Hash of the original CommitmentRequest (binds commitment to request)
    request_hash VARCHAR(66) UNIQUE NOT NULL,

    -- Core commitment fields
    commitment_type BIGINT NOT NULL,
    payload BYTEA NOT NULL,
    slasher VARCHAR(42) NOT NULL,  -- Ethereum address (0x + 40 hex chars)

    -- ECDSA signature over the commitment
    signature VARCHAR(132) NOT NULL,  -- ECDSA signature (0x + 130 hex chars)

    -- Additional metadata
    created_at TIMESTAMPTZ DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_request_hash_format CHECK (request_hash ~ '^0x[a-fA-F0-9]{64}$'),
    CONSTRAINT chk_slasher_format CHECK (slasher ~ '^0x[a-fA-F0-9]{40}$'),
    CONSTRAINT chk_signature_format CHECK (signature ~ '^0x[a-fA-F0-9]{130}$'),
    CONSTRAINT chk_commitment_type_positive CHECK (commitment_type > 0)
);

-- Indexes for efficient querying
CREATE INDEX idx_commitments_request_hash ON commitments(request_hash);
CREATE INDEX idx_commitments_commitment_type ON commitments(commitment_type);
CREATE INDEX idx_commitments_slasher ON commitments(slasher);
CREATE INDEX idx_commitments_created_at ON commitments(created_at DESC);

-- Comments for documentation
COMMENT ON TABLE commitments IS 'Stores signed commitments according to the Gateway specification';
COMMENT ON COLUMN commitments.request_hash IS 'Keccak256 hash of the original CommitmentRequest, binding this commitment to the request';
COMMENT ON COLUMN commitments.commitment_type IS 'Type identifier for the commitment (uint64 from spec)';
COMMENT ON COLUMN commitments.payload IS 'Opaque bytes containing the commitment payload';
COMMENT ON COLUMN commitments.slasher IS 'Ethereum address of the slasher contract for dispute resolution';
COMMENT ON COLUMN commitments.signature IS 'ECDSA signature over keccak256(abi.encode(commitment))';