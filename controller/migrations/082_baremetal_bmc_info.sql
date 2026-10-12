-- What a server's BMC reports (Redfish): identity, health, temperatures, power draw,
-- firmware versions and recent log entries. A snapshot, replaced on each refresh.
CREATE TABLE baremetal_bmc_info (
    server_id TEXT PRIMARY KEY NOT NULL REFERENCES baremetal_servers(id) ON DELETE CASCADE,
    health TEXT NOT NULL DEFAULT '',
    info_json TEXT NOT NULL DEFAULT '{}',
    collected_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
