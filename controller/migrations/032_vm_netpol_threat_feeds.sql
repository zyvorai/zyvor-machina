-- DNS threat feeds the controller keeps on every host's machina-bpfd.
CREATE TABLE IF NOT EXISTS vm_netpol_threat_feeds (
    name TEXT NOT NULL PRIMARY KEY,
    -- URL it is fetched from; empty for an inline list
    source TEXT NOT NULL DEFAULT '',
    block INTEGER NOT NULL DEFAULT 0,
    -- JSON array of normalized domains
    domains TEXT NOT NULL DEFAULT '[]',
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
