-- Platform batch 6: API keys, webhooks, maintenance schedules, extensions

CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'operator',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS webhooks (
    id UUID PRIMARY KEY,
    url TEXT NOT NULL,
    events TEXT[] NOT NULL DEFAULT '{}',
    secret TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS maintenance_schedules (
    id UUID PRIMARY KEY,
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    action TEXT NOT NULL DEFAULT 'enter',
    evacuate BOOLEAN NOT NULL DEFAULT TRUE,
    run_at TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS notification_outbox (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}',
    delivered BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE snapshot_records ADD COLUMN IF NOT EXISTS snapshot_path TEXT NOT NULL DEFAULT '';
ALTER TABLE backup_records ADD COLUMN IF NOT EXISTS backup_path TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_maintenance_sched_run ON maintenance_schedules(run_at) WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
