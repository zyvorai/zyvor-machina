-- Deploy profiles: what to put on machines of a kind (image, keys, zone, size), and an
-- optional desired count that the controller keeps reconciling to when auto_deploy is on.
CREATE TABLE baremetal_profiles (
    name TEXT PRIMARY KEY NOT NULL,
    image TEXT NOT NULL,
    ssh_keys TEXT NOT NULL DEFAULT '[]',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    zone TEXT NOT NULL DEFAULT '',
    min_cpu_cores INTEGER NOT NULL DEFAULT 0,
    min_memory_mib INTEGER NOT NULL DEFAULT 0,
    auto_deploy INTEGER NOT NULL DEFAULT 0,
    desired_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE baremetal_servers ADD COLUMN profile TEXT NOT NULL DEFAULT '';
