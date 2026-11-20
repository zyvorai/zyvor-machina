-- Trust ladder: per action class, whether Zyra must ask (default) or may run it by itself in autopilot mode.
CREATE TABLE IF NOT EXISTS ai_trust_rules (
    action_type TEXT NOT NULL PRIMARY KEY,
    level TEXT NOT NULL DEFAULT 'ask' CHECK (level IN ('ask', 'auto')),
    max_per_run INTEGER NOT NULL DEFAULT 3,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
