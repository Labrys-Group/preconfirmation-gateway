-- Migration 006: Create slot congestion tracking table
-- This table tracks gas usage and congestion metrics per slot for dynamic fee calculation

CREATE TABLE slot_congestion (
    id SERIAL PRIMARY KEY,
    slot BIGINT NOT NULL UNIQUE,

    -- Gas usage tracking
    preconfirmed_gas BIGINT NOT NULL DEFAULT 0,
    total_gas_limit BIGINT NOT NULL DEFAULT 30000000,
    gas_used_ratio DECIMAL(5,4) NOT NULL DEFAULT 0.0000,

    -- Fee calculation metadata
    base_gas_price BIGINT NOT NULL,
    calculated_fee_multiplier DECIMAL(10,6) NOT NULL DEFAULT 1.0,
    current_tx_price BIGINT NOT NULL,

    -- Timestamp tracking
    slot_start_time TIMESTAMP NOT NULL,
    last_updated TIMESTAMP DEFAULT NOW(),
    created_at TIMESTAMP DEFAULT NOW()
);

-- Indexes for efficient queries
CREATE INDEX idx_slot_congestion_slot ON slot_congestion(slot);
CREATE INDEX idx_slot_congestion_slot_start_time ON slot_congestion(slot_start_time);
CREATE INDEX idx_slot_congestion_gas_ratio ON slot_congestion(gas_used_ratio);

-- Add comments for documentation
COMMENT ON TABLE slot_congestion IS 'Tracks gas usage and congestion metrics per slot for dynamic fee calculation';
COMMENT ON COLUMN slot_congestion.slot IS 'Ethereum slot number (12-second intervals)';
COMMENT ON COLUMN slot_congestion.preconfirmed_gas IS 'Total gas already preconfirmed in this slot';
COMMENT ON COLUMN slot_congestion.total_gas_limit IS 'Maximum gas limit for this slot (typically 30M)';
COMMENT ON COLUMN slot_congestion.gas_used_ratio IS 'Ratio of preconfirmed_gas / total_gas_limit (0.0 to 1.0)';
COMMENT ON COLUMN slot_congestion.base_gas_price IS 'Base gas price from Reth oracle (in wei)';
COMMENT ON COLUMN slot_congestion.calculated_fee_multiplier IS 'Multiplier from congestion formula: 1 / (1 - ratio^k)';
COMMENT ON COLUMN slot_congestion.current_tx_price IS 'Final transaction price: base_price * multiplier (in wei)';
COMMENT ON COLUMN slot_congestion.slot_start_time IS 'When this slot started (for cleanup purposes)';