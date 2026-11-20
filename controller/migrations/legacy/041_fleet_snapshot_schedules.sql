-- Fleet snapshot schedules (Phase B)
CREATE TABLE IF NOT EXISTS fleet_snapshot_schedules (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    cron_expr TEXT NOT NULL DEFAULT '0 2 * * *',
    project TEXT NOT NULL DEFAULT '',
    tag_filter TEXT NOT NULL DEFAULT '',
    disk_only BOOLEAN NOT NULL DEFAULT TRUE,
    quiesce BOOLEAN NOT NULL DEFAULT FALSE,
    retain_count INT NOT NULL DEFAULT 5,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fleet_snapshot_schedules_enabled ON fleet_snapshot_schedules(enabled);
