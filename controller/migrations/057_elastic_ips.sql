-- Elastic IPs: pools of addresses a host holds on an interface, and 1:1 associations with instances on that host.
CREATE TABLE IF NOT EXISTS eip_pools (
    id BLOB NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    cidr TEXT NOT NULL,
    host_id BLOB NOT NULL,
    interface TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS elastic_ips (
    id BLOB NOT NULL PRIMARY KEY,
    pool_id BLOB NOT NULL REFERENCES eip_pools(id) ON DELETE RESTRICT,
    address TEXT NOT NULL UNIQUE,
    vm_id BLOB,
    allocated_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    associated_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_elastic_ips_vm ON elastic_ips(vm_id);
