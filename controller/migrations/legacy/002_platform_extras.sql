-- Platform extras: enrollment, console addr, storage/network inventory

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS agent_console_addr TEXT NOT NULL DEFAULT '127.0.0.1:50052';

CREATE TABLE IF NOT EXISTS storage_pools (
    id UUID PRIMARY KEY,
    cluster_id UUID REFERENCES clusters(id),
    name TEXT NOT NULL,
    storage_class TEXT NOT NULL DEFAULT 'silver',
    backend TEXT NOT NULL DEFAULT 'directory',
    path TEXT,
    capacity_gib BIGINT NOT NULL DEFAULT 0,
    used_gib BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (cluster_id, name)
);

CREATE TABLE IF NOT EXISTS networks (
    id UUID PRIMARY KEY,
    cluster_id UUID REFERENCES clusters(id),
    name TEXT NOT NULL,
    backend TEXT NOT NULL DEFAULT 'linux-bridge',
    vlan_id INT,
    bridge TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (cluster_id, name)
);
