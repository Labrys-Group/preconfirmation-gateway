-- Fix delegation_id to be NOT NULL in constraint_submissions table
-- This ensures referential integrity and makes the unique constraint effective

-- Step 1: Delete any orphaned submissions without a delegation
DELETE FROM constraint_submissions WHERE delegation_id IS NULL;

-- Step 2: Make delegation_id NOT NULL
ALTER TABLE constraint_submissions
    ALTER COLUMN delegation_id SET NOT NULL;

-- Comment the change
COMMENT ON COLUMN constraint_submissions.delegation_id IS 'Delegation that authorized this constraint submission (required)';
