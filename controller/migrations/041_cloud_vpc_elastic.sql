-- VPC networking is project-owned and host-local in v1. Route and peering
-- records are plans, not evidence of active forwarding.
CREATE TABLE IF NOT EXISTS cloud_vpcs (
 id TEXT PRIMARY KEY NOT NULL,
 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
 host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE RESTRICT,
 name TEXT NOT NULL, cidr TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE(project_id, name)
);
CREATE TABLE IF NOT EXISTS cloud_subnets (
 id TEXT PRIMARY KEY NOT NULL,
 vpc_id TEXT NOT NULL REFERENCES cloud_vpcs(id) ON DELETE RESTRICT,
 network_id TEXT NOT NULL UNIQUE REFERENCES networks(id) ON DELETE RESTRICT,
 name TEXT NOT NULL, cidr TEXT NOT NULL,
 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','ready','error')),
 last_error TEXT NOT NULL DEFAULT '',
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE(vpc_id, name), UNIQUE(vpc_id, cidr)
);
CREATE TABLE IF NOT EXISTS cloud_ip_allocations (
 id TEXT PRIMARY KEY NOT NULL,
 subnet_id TEXT NOT NULL REFERENCES cloud_subnets(id) ON DELETE RESTRICT,
 request_key TEXT NOT NULL, address TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE(subnet_id, request_key), UNIQUE(subnet_id, address)
);
CREATE TABLE IF NOT EXISTS cloud_routes (
 id TEXT PRIMARY KEY NOT NULL,
 vpc_id TEXT NOT NULL REFERENCES cloud_vpcs(id) ON DELETE CASCADE,
 destination TEXT NOT NULL,
 target TEXT NOT NULL CHECK(target IN ('blackhole','local','peering')),
 target_id TEXT,
 UNIQUE(vpc_id, destination)
);
CREATE TABLE IF NOT EXISTS cloud_peerings (
 id TEXT PRIMARY KEY NOT NULL,
 requester_id TEXT NOT NULL REFERENCES cloud_vpcs(id) ON DELETE RESTRICT,
 accepter_id TEXT NOT NULL REFERENCES cloud_vpcs(id) ON DELETE RESTRICT,
 status TEXT NOT NULL DEFAULT 'pending_acceptance' CHECK(status IN ('pending_acceptance','planned')),
 CHECK(requester_id != accepter_id), UNIQUE(requester_id, accepter_id)
);
CREATE TABLE IF NOT EXISTS cloud_launch_templates (
 id TEXT PRIMARY KEY NOT NULL,
 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
 name TEXT NOT NULL, spec_json TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE(project_id, name)
);
CREATE TABLE IF NOT EXISTS cloud_instance_groups (
 id TEXT PRIMARY KEY NOT NULL,
 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
 template_id TEXT NOT NULL REFERENCES cloud_launch_templates(id) ON DELETE RESTRICT,
 subnet_id TEXT NOT NULL REFERENCES cloud_subnets(id) ON DELETE RESTRICT,
 name TEXT NOT NULL, policy_json TEXT NOT NULL,
 paused INTEGER NOT NULL DEFAULT 0,
 last_scaled_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 last_error TEXT NOT NULL DEFAULT '',
 UNIQUE(project_id, name)
);
CREATE TABLE IF NOT EXISTS cloud_group_members (
 group_id TEXT NOT NULL REFERENCES cloud_instance_groups(id) ON DELETE RESTRICT,
 slot INTEGER NOT NULL CHECK(slot >= 0),
 vm_id TEXT UNIQUE REFERENCES vms(id) ON DELETE SET NULL,
 PRIMARY KEY(group_id, slot)
);
CREATE INDEX IF NOT EXISTS cloud_subnets_vpc ON cloud_subnets(vpc_id);
CREATE INDEX IF NOT EXISTS cloud_group_project ON cloud_instance_groups(project_id);
