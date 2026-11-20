-- LLDP neighbor cache for topology + digital twin (hardening slice)

CREATE TABLE IF NOT EXISTS host_lldp_cache (
    host_id UUID PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT '',
    neighbors_json JSONB NOT NULL DEFAULT '[]',
    summary TEXT NOT NULL DEFAULT '',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_host_lldp_cache_fetched ON host_lldp_cache(fetched_at DESC);
