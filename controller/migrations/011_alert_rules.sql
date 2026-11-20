-- Day-2: user-defined metric-threshold alert rules. Previously alerts were only
-- event-emitted to signed webhooks; operators could not say "CPU > 90% → notify".
-- Rules are evaluated over vm_metrics and emitted through the existing
-- notification_outbox (so the webhook_worker delivers them), with a per-rule cooldown.
CREATE TABLE IF NOT EXISTS alert_rules (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    metric TEXT NOT NULL DEFAULT 'cpu_percent',   -- 'cpu_percent' | 'mem_percent'
    comparator TEXT NOT NULL DEFAULT 'gt',         -- 'gt' | 'lt'
    threshold REAL NOT NULL DEFAULT 90,
    severity TEXT NOT NULL DEFAULT 'warning',
    scope_project TEXT NOT NULL DEFAULT '',
    scope_tag TEXT NOT NULL DEFAULT '',
    cooldown_minutes INTEGER NOT NULL DEFAULT 30,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_fired_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_alert_rules_enabled ON alert_rules(enabled);
