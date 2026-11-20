-- Native eBPF (machina-bpfd) replaces Tetragon, PacketWolf and Netra.
-- Runtime enforcement policies are owned by the controller and pushed to each
-- host's machina-bpfd through the agent.

CREATE TABLE IF NOT EXISTS bpf_policies (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL,
    match_value TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    -- 'fleet', 'host:<host id>', 'vm:<name>' or 'cgroup:<path>'
    scope TEXT NOT NULL DEFAULT 'fleet',
    description TEXT NOT NULL DEFAULT '',
    applied_hosts TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

DROP TABLE IF EXISTS packetwolf_local_sensors;

UPDATE platform_plugins
SET slug = 'native-bpf',
    name = 'Native eBPF',
    description = 'Kernel-native flows, process telemetry, enforcement, capture and QoS (machina-bpfd).',
    installed = 1
WHERE slug = 'packetwolf';

UPDATE soc_detection_rules
SET description = 'Native eBPF critical or high severity anomaly',
    query_json = '{"type":"match","match":{"source":"machina-bpf","severity":["critical","high"]}}'
WHERE name = 'critical_anomaly';

DELETE FROM soc_ingest_watermarks WHERE source = 'packetwolf';
INSERT INTO soc_ingest_watermarks (source, last_at) VALUES ('machina-bpf', '1970-01-01T00:00:00Z')
ON CONFLICT (source) DO NOTHING;
