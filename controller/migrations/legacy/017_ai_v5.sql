-- AI v5: scheduled Autopilot + compliance PDF metadata
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS ai_autopilot_interval_secs INT NOT NULL DEFAULT 0;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS ai_autopilot_last_run TIMESTAMPTZ;
