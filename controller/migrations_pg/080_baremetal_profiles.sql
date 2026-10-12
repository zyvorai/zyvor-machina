-- Deploy profiles: what to put on machines of a kind (image, keys, zone, size), and an
-- optional desired count that the controller keeps reconciling to when auto_deploy is on.
CREATE TABLE baremetal_profiles (
    name TEXT PRIMARY KEY NOT NULL,
    image TEXT NOT NULL,
    ssh_keys TEXT NOT NULL DEFAULT '[]',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    zone TEXT NOT NULL DEFAULT '',
    min_cpu_cores BIGINT NOT NULL DEFAULT 0,
    min_memory_mib BIGINT NOT NULL DEFAULT 0,
    auto_deploy BOOLEAN NOT NULL DEFAULT FALSE,
    desired_count BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);
ALTER TABLE baremetal_servers ADD COLUMN profile TEXT NOT NULL DEFAULT '';
