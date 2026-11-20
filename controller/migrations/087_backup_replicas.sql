-- Off-site copies of backups: one row per (backup, S3 target). A backup is copied by the agent on
-- the host that holds it and verified by comparing object sizes with the files on disk.
-- Status: running | verified | failed. Fetch records the last time a copy was pulled back to a
-- host (for a restore at another site); status: '' | running | done | failed.
CREATE TABLE backup_replicas (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) = 16),
    backup_id TEXT NOT NULL REFERENCES backup_records(id) ON DELETE CASCADE,
    target_id TEXT NOT NULL REFERENCES backup_targets(id) ON DELETE CASCADE,
    remote_uri TEXT NOT NULL DEFAULT '',
    bytes INTEGER NOT NULL DEFAULT 0,
    files INTEGER NOT NULL DEFAULT 0,
    is_dir INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'running',
    message TEXT NOT NULL DEFAULT '',
    verified_at TEXT NOT NULL DEFAULT '',
    fetch_status TEXT NOT NULL DEFAULT '',
    fetch_path TEXT NOT NULL DEFAULT '',
    fetch_message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (backup_id, target_id)
);
CREATE INDEX idx_backup_replicas_target ON backup_replicas(target_id, status);
