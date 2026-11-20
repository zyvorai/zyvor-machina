-- Security groups and ports — Phase B of the next-gen roadmap
-- (/Users/ssahani/.claude/plans/lazy-munching-quilt.md). Native /api/v1/... resources,
-- not wire-compatible with any external cloud API. A port bound to a VM (vm_id set) delegates the actual
-- NIC attach/detach to the existing vms::attach_vm_nic/detach_vm_nic — no new libvirt
-- plumbing here.
--
-- Security-group RULE ENFORCEMENT is not wired to engine::zeus_firewall in this first
-- cut — groups/rules are real, stored, and attachable, but purely advisory until that
-- integration is built. Flagged here rather than silently implied as working.
CREATE TABLE IF NOT EXISTS security_groups (
    id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS security_group_rules (
    id TEXT NOT NULL PRIMARY KEY,
    security_group_id TEXT NOT NULL REFERENCES security_groups(id) ON DELETE CASCADE,
    direction TEXT NOT NULL DEFAULT 'ingress',
    protocol TEXT,
    port_min INTEGER,
    port_max INTEGER,
    remote_cidr TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_secgroup_rules_group ON security_group_rules(security_group_id);

CREATE TABLE IF NOT EXISTS ports (
    id TEXT NOT NULL PRIMARY KEY,
    network_id TEXT NOT NULL REFERENCES networks(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    vm_id TEXT REFERENCES vms(id) ON DELETE SET NULL,
    mac_address TEXT,
    security_group_id TEXT REFERENCES security_groups(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'DOWN',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_ports_network ON ports(network_id);
CREATE INDEX IF NOT EXISTS idx_ports_vm ON ports(vm_id);
