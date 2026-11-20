-- EC2 keeps a terminated instance visible for about an hour. The tombstone is written by the delete task before the
-- instance row goes; tags stay until it expires.
CREATE TABLE IF NOT EXISTS terminated_instances (
    id BLOB NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    instance_type TEXT,
    project TEXT,
    vcpus INTEGER NOT NULL DEFAULT 0,
    memory_mib INTEGER NOT NULL DEFAULT 0,
    terminated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
