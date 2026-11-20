-- HA recovery tracking

CREATE TABLE IF NOT EXISTS ha_events (
    id UUID PRIMARY KEY,
    vm_id UUID REFERENCES vms(id) ON DELETE SET NULL,
    host_id UUID REFERENCES hosts(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE vms ADD COLUMN IF NOT EXISTS ha_recovery_count INT NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS placement_recommendations (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    from_host_id UUID NOT NULL REFERENCES hosts(id),
    to_host_id UUID NOT NULL REFERENCES hosts(id),
    reason TEXT NOT NULL,
    score REAL NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ha_events_created ON ha_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_placement_open ON placement_recommendations(status) WHERE status = 'open';
