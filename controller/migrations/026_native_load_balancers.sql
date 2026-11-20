-- Native L4 load balancer — the last piece of Fleet Cloud's native compute layer.
-- Unlike a typical amphora-based load balancer (VMs running full HAProxy instances),
-- this is kernel-level: one hypervisor host installs a weighted round-robin DNAT rule
-- set (iptables `statistic` match, see core::libvirt::host_network::set_load_balancer_rules)
-- that fans out `listener_port` on that host to N backend VM:port members. No amphora,
-- no separate LB VM, no external cloud dependency.
--
-- Scope is deliberately L4 only (no L7 policies, no amphora diagnostics). Member health is a
-- manual `enabled` toggle for v1, not an automatic health monitor -- see load_balancer.rs.
CREATE TABLE IF NOT EXISTS load_balancers (
    id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'tcp',
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    listener_port INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    status_message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (host_id, protocol, listener_port)
);

CREATE TABLE IF NOT EXISTS lb_members (
    id TEXT NOT NULL PRIMARY KEY,
    load_balancer_id TEXT NOT NULL REFERENCES load_balancers(id) ON DELETE CASCADE,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    port INTEGER NOT NULL,
    weight INTEGER NOT NULL DEFAULT 1,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (load_balancer_id, vm_id, port)
);
CREATE INDEX IF NOT EXISTS idx_lb_members_lb ON lb_members(load_balancer_id);
