-- Create constraints tracking table for managing constraint submissions
-- This table tracks the state of constraint messages sent to the relay

CREATE TABLE constraint_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Reference to the delegation that authorized this constraint
    delegation_id UUID REFERENCES delegations(id),

    -- Slot and constraint message information
    slot_number BIGINT NOT NULL,
    proposer_pubkey BYTEA NOT NULL,     -- BLS public key of the proposer (48 bytes)
    delegate_pubkey BYTEA NOT NULL,     -- BLS public key of the Gateway (48 bytes)

    -- Constraint message data
    constraints_message JSONB NOT NULL, -- Full ConstraintsMessage as JSON
    bls_signature BYTEA NOT NULL,       -- BLS signature over the message (96 bytes)

    -- Submission tracking
    submission_status VARCHAR(50) NOT NULL DEFAULT 'pending',
    relay_endpoint VARCHAR(255),        -- Which relay endpoint was used
    submitted_at TIMESTAMPTZ,          -- When constraint was submitted
    response_status INTEGER,           -- HTTP response status from relay
    response_body TEXT,                -- Response body from relay (for debugging)

    -- Timing information
    created_at TIMESTAMPTZ DEFAULT NOW(),
    deadline_at TIMESTAMPTZ NOT NULL,  -- 8-second deadline for this slot

    -- Constraints
    CONSTRAINT chk_proposer_pubkey_length CHECK (LENGTH(proposer_pubkey) = 48),
    CONSTRAINT chk_delegate_pubkey_length CHECK (LENGTH(delegate_pubkey) = 48),
    CONSTRAINT chk_bls_signature_length CHECK (LENGTH(bls_signature) = 96),
    CONSTRAINT chk_slot_positive CHECK (slot_number >= 0),
    CONSTRAINT chk_submission_status CHECK (submission_status IN ('pending', 'submitted', 'failed', 'too_late')),

    -- One submission per slot per delegation
    CONSTRAINT unique_constraint_per_slot_delegation UNIQUE (delegation_id, slot_number)
);

-- Indexes for efficient querying
CREATE INDEX idx_constraints_slot ON constraint_submissions(slot_number);
CREATE INDEX idx_constraints_status ON constraint_submissions(submission_status);
CREATE INDEX idx_constraints_deadline ON constraint_submissions(deadline_at);
CREATE INDEX idx_constraints_submitted_at ON constraint_submissions(submitted_at);
CREATE INDEX idx_constraints_pending ON constraint_submissions(slot_number, submission_status)
    WHERE submission_status = 'pending';

-- Create a table to track individual constraints within a submission
CREATE TABLE constraint_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Reference to the parent constraint submission
    submission_id UUID REFERENCES constraint_submissions(id) ON DELETE CASCADE,

    -- Reference to the commitment that generated this constraint
    commitment_id UUID REFERENCES commitments(id),

    -- Constraint details
    constraint_type BIGINT NOT NULL,
    constraint_payload BYTEA NOT NULL,

    -- Order within the constraints message (important for processing order)
    order_index INTEGER NOT NULL,

    created_at TIMESTAMPTZ DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_constraint_type_positive CHECK (constraint_type > 0),
    CONSTRAINT chk_order_non_negative CHECK (order_index >= 0),

    -- Unique ordering within a submission
    CONSTRAINT unique_order_per_submission UNIQUE (submission_id, order_index)
);

-- Indexes for constraint items
CREATE INDEX idx_constraint_items_submission ON constraint_items(submission_id);
CREATE INDEX idx_constraint_items_commitment ON constraint_items(commitment_id);
CREATE INDEX idx_constraint_items_type ON constraint_items(constraint_type);

-- Comments for documentation
COMMENT ON TABLE constraint_submissions IS 'Tracks constraint message submissions to the relay';
COMMENT ON COLUMN constraint_submissions.delegation_id IS 'Delegation that authorized this constraint submission';
COMMENT ON COLUMN constraint_submissions.constraints_message IS 'Complete ConstraintsMessage as JSON for auditability';
COMMENT ON COLUMN constraint_submissions.submission_status IS 'Current status: pending, submitted, failed, too_late';
COMMENT ON COLUMN constraint_submissions.deadline_at IS '8-second deadline timestamp for this slot';

COMMENT ON TABLE constraint_items IS 'Individual constraints within a constraint submission';
COMMENT ON COLUMN constraint_items.order_index IS 'Processing order within the constraints message (0-based)';
COMMENT ON COLUMN constraint_items.constraint_payload IS 'Raw constraint payload bytes';