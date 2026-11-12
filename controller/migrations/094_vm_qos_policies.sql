-- Neutron-ish QoS: bandwidth / PPS policies applied to VM edge via bpfd.
CREATE TABLE IF NOT EXISTS vm_qos_policies (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    project_id TEXT,
    egress_mbps INTEGER,
    ingress_mbps INTEGER,
    pps INTEGER,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS vm_qos_bindings (
    id TEXT NOT NULL PRIMARY KEY,
    policy_id TEXT NOT NULL REFERENCES vm_qos_policies(id) ON DELETE CASCADE,
    vm_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(vm_id)
);
CREATE INDEX IF NOT EXISTS idx_vm_qos_bindings_policy ON vm_qos_bindings (policy_id);
