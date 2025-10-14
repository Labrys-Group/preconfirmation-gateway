-- Add slot column to commitments table for constraint submission queries
-- This enables the constraint submission service to find commitments by slot

-- Add slot_number column (nullable initially to handle existing data)
ALTER TABLE commitments
ADD COLUMN slot_number BIGINT;

-- Add processed flag to track which commitments have been converted to constraints
ALTER TABLE commitments
ADD COLUMN constraint_processed BOOLEAN NOT NULL DEFAULT FALSE;

-- Create index for efficient slot-based queries
CREATE INDEX idx_commitments_slot_number ON commitments(slot_number);

-- Create composite index for finding unprocessed commitments by slot
CREATE INDEX idx_commitments_slot_unprocessed ON commitments(slot_number, constraint_processed)
    WHERE constraint_processed = FALSE;

-- Add check constraint to ensure slot numbers are positive when present
ALTER TABLE commitments
ADD CONSTRAINT chk_slot_number_positive CHECK (slot_number IS NULL OR slot_number >= 0);

-- Comments for documentation
COMMENT ON COLUMN commitments.slot_number IS 'Ethereum slot number extracted from the commitment payload';
COMMENT ON COLUMN commitments.constraint_processed IS 'Whether this commitment has been converted to a constraint and submitted to relay';
