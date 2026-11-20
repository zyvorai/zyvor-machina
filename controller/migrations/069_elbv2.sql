-- ELBv2 (service `elasticloadbalancing`). A listener with a forward action is realised as one native L4 balancer
-- (`load_balancers`, iptables on the ELBv2 balancer's host) whose `lb_members` mirror the target group's targets.
CREATE TABLE elbv2_load_balancers (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    lb_type TEXT NOT NULL DEFAULT 'network',
    scheme TEXT NOT NULL DEFAULT 'internet-facing',
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    subnet_ids TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE elbv2_target_groups (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    protocol TEXT NOT NULL DEFAULT 'TCP',
    port INTEGER NOT NULL,
    target_type TEXT NOT NULL DEFAULT 'instance',
    vpc_id TEXT NOT NULL DEFAULT '',
    hc_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    hc_protocol TEXT NOT NULL DEFAULT 'TCP',
    hc_port TEXT NOT NULL DEFAULT 'traffic-port',
    hc_path TEXT NOT NULL DEFAULT '/',
    hc_interval_secs INTEGER NOT NULL DEFAULT 30,
    hc_timeout_secs INTEGER NOT NULL DEFAULT 10,
    hc_healthy_threshold INTEGER NOT NULL DEFAULT 3,
    hc_unhealthy_threshold INTEGER NOT NULL DEFAULT 3,
    hc_matcher TEXT NOT NULL DEFAULT '200',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE elbv2_targets (
    id TEXT NOT NULL PRIMARY KEY,
    target_group_id TEXT NOT NULL REFERENCES elbv2_target_groups(id) ON DELETE CASCADE,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    port INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (target_group_id, vm_id, port)
);
CREATE INDEX idx_elbv2_targets_group ON elbv2_targets (target_group_id);

CREATE TABLE elbv2_listeners (
    id TEXT NOT NULL PRIMARY KEY,
    load_balancer_id TEXT NOT NULL REFERENCES elbv2_load_balancers(id) ON DELETE CASCADE,
    protocol TEXT NOT NULL,
    port INTEGER NOT NULL,
    target_group_id TEXT NOT NULL REFERENCES elbv2_target_groups(id),
    native_lb_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (load_balancer_id, port)
);
CREATE INDEX idx_elbv2_listeners_group ON elbv2_listeners (target_group_id);

CREATE TABLE elbv2_attributes (
    resource_id TEXT NOT NULL,
    attr_key TEXT NOT NULL,
    attr_value TEXT NOT NULL,
    PRIMARY KEY (resource_id, attr_key)
);

CREATE TABLE elbv2_tags (
    resource_id TEXT NOT NULL,
    tag_key TEXT NOT NULL,
    tag_value TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (resource_id, tag_key)
);
