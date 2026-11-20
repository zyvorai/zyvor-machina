-- Scheduled backups with retention (day-2). Mirrors fleet_snapshot_schedules, which
-- already has scheduling + retention; backups previously had neither (manual-trigger only).
CREATE TABLE IF NOT EXISTS backup_schedules (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    project TEXT NOT NULL DEFAULT '',
    tag_filter TEXT NOT NULL DEFAULT '',
    backup_type TEXT NOT NULL DEFAULT 'full',
    target_id TEXT,
    interval_hours INTEGER NOT NULL DEFAULT 24,
    retain_count INTEGER NOT NULL DEFAULT 7,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_backup_schedules_enabled ON backup_schedules(enabled);
