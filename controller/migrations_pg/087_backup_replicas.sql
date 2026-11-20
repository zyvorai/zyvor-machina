-- Off-site copies of backups: one row per (backup, S3 target). A backup is copied by the agent on
-- the host that holds it and verified by comparing object sizes with the files on disk.
-- Status: running | verified | failed. Fetch records the last time a copy was pulled back to a
-- host (for a restore at another site); status: '' | running | done | failed.
CREATE TABLE backup_replicas (
    id UUID PRIMARY KEY NOT NULL,
    backup_id UUID NOT NULL REFERENCES backup_records(id) ON DELETE CASCADE,
    target_id UUID NOT NULL REFERENCES backup_targets(id) ON DELETE CASCADE,
    remote_uri TEXT NOT NULL DEFAULT '',
    bytes BIGINT NOT NULL DEFAULT 0,
    files BIGINT NOT NULL DEFAULT 0,
    is_dir BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL DEFAULT 'running',
    message TEXT NOT NULL DEFAULT '',
    verified_at TEXT NOT NULL DEFAULT '',
    fetch_status TEXT NOT NULL DEFAULT '',
    fetch_path TEXT NOT NULL DEFAULT '',
    fetch_message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE (backup_id, target_id)
);
CREATE INDEX idx_backup_replicas_target ON backup_replicas(target_id, status);
