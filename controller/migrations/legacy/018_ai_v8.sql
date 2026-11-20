-- Zeus OS v8: Autopilot v2 max actions + fleet peer URLs for multi-cluster summary
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS ai_autopilot_max_actions INT NOT NULL DEFAULT 5;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS ai_fleet_peer_urls JSONB NOT NULL DEFAULT '[]'::jsonb;
