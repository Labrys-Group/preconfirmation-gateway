-- Fix submission_id and commitment_id to be NOT NULL in constraint_items table
-- This ensures every constraint item references a real submission and commitment

-- Step 1: Delete any orphaned constraint items without a submission
DELETE FROM constraint_items WHERE submission_id IS NULL;

-- Step 2: Delete any constraint items without a commitment
DELETE FROM constraint_items WHERE commitment_id IS NULL;

-- Step 3: Make submission_id NOT NULL
ALTER TABLE constraint_items
    ALTER COLUMN submission_id SET NOT NULL;

-- Step 4: Make commitment_id NOT NULL
ALTER TABLE constraint_items
    ALTER COLUMN commitment_id SET NOT NULL;

-- Comment the changes
COMMENT ON COLUMN constraint_items.submission_id IS 'Parent constraint submission (required)';
COMMENT ON COLUMN constraint_items.commitment_id IS 'Commitment that generated this constraint (required)';
