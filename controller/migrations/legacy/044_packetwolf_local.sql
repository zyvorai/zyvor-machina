-- Controller-local PacketWolf sensor registry (production fabric API fallback)

CREATE TABLE IF NOT EXISTS packetwolf_local_sensors (
    host_id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'registered',
    tetragon_version TEXT NOT NULL DEFAULT '1.7.0',
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_event_at TIMESTAMPTZ,
    pending_tetragon JSONB
);

CREATE INDEX IF NOT EXISTS idx_packetwolf_local_sensors_status
    ON packetwolf_local_sensors (status);
