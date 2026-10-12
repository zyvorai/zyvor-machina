-- Bare-metal lifecycle (new -> commissioning -> ready -> allocated -> deploying -> deployed
-- -> retired, plus failed), zones, the MAC used to match a PXE-booting machine, a one-time
-- commissioning token (stored hashed), and the hardware report a commissioning boot posts.
ALTER TABLE baremetal_servers ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'new';
ALTER TABLE baremetal_servers ADD COLUMN zone TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN mac_address TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN commission_token_hash TEXT NOT NULL DEFAULT '';
CREATE INDEX idx_baremetal_servers_lifecycle ON baremetal_servers(lifecycle, zone);
CREATE TABLE baremetal_inventory (
    server_id UUID PRIMARY KEY NOT NULL REFERENCES baremetal_servers(id) ON DELETE CASCADE,
    report_json TEXT NOT NULL DEFAULT '{}',
    cpu_cores BIGINT NOT NULL DEFAULT 0,
    memory_mib BIGINT NOT NULL DEFAULT 0,
    disk_count BIGINT NOT NULL DEFAULT 0,
    nic_count BIGINT NOT NULL DEFAULT 0,
    collected_at TEXT NOT NULL DEFAULT (to_char((now() AT TIME ZONE 'utc'), 'YYYY-MM-DD HH24:MI:SS'))
);
