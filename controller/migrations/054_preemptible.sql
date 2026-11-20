-- Preemptible instances: under memory pressure they are managed-saved
-- (desired_state='sleeping', preempted_at set) instead of terminated, and
-- restored when their host has room again. Lower priority goes first.
ALTER TABLE vms ADD COLUMN preemptible INTEGER NOT NULL DEFAULT 0;
ALTER TABLE vms ADD COLUMN preempt_priority INTEGER NOT NULL DEFAULT 0;
ALTER TABLE vms ADD COLUMN preempted_at TEXT;

CREATE TABLE IF NOT EXISTS preempt_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 1,
    -- Free memory each host keeps, as a percent of its total.
    reserve_pct INTEGER NOT NULL DEFAULT 10,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT OR IGNORE INTO preempt_settings (id) VALUES (1);
