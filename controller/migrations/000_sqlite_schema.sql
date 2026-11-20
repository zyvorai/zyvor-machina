-- Machina controller consolidated SQLite schema (generated from migrations 001-044)

-- ============================================================
-- clusters
-- ============================================================
CREATE TABLE IF NOT EXISTS clusters (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    ha_enabled INTEGER NOT NULL DEFAULT 0,
    placement_policy TEXT NOT NULL DEFAULT 'balanced',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 004
    drs_auto_migrate INTEGER NOT NULL DEFAULT 0,
    drs_cpu_threshold REAL NOT NULL DEFAULT 85.0,
    -- 007
    oidc_enabled INTEGER NOT NULL DEFAULT 0,
    oidc_issuer TEXT NOT NULL DEFAULT '',
    oidc_client_id TEXT NOT NULL DEFAULT '',
    cpu_compat_matrix TEXT NOT NULL DEFAULT '[]',
    -- 008
    oidc_client_secret TEXT NOT NULL DEFAULT '',
    oidc_redirect_uri TEXT NOT NULL DEFAULT '',
    -- 009
    inventory_sync_interval_secs INTEGER NOT NULL DEFAULT 30,
    -- 015
    require_vm_delete_approval INTEGER NOT NULL DEFAULT 0,
    finops_vcpu_hour_usd REAL NOT NULL DEFAULT 0.02,
    finops_gib_hour_usd REAL NOT NULL DEFAULT 0.005,
    -- 016
    ai_enabled INTEGER NOT NULL DEFAULT 0,
    ai_mode TEXT NOT NULL DEFAULT 'advisor',
    ai_provider TEXT NOT NULL DEFAULT 'openai',
    ai_model TEXT NOT NULL DEFAULT 'gpt-4o-mini',
    ai_api_key TEXT NOT NULL DEFAULT '',
    -- 017
    ai_autopilot_interval_secs INTEGER NOT NULL DEFAULT 0,
    ai_autopilot_last_run TEXT,
    -- 018
    ai_autopilot_max_actions INTEGER NOT NULL DEFAULT 5,
    ai_fleet_peer_urls TEXT NOT NULL DEFAULT '[]',
    -- 023
    firewall_approval_sla_hours INTEGER NOT NULL DEFAULT 72,
    -- 035
    inventory_prune_unmanaged INTEGER NOT NULL DEFAULT 1,
    inventory_mark_managed_missing INTEGER NOT NULL DEFAULT 1,
    -- 036
    zeus_multi_provider INTEGER NOT NULL DEFAULT 1,
    zeus_agents_enabled INTEGER NOT NULL DEFAULT 1,
    zeus_ambient_ux INTEGER NOT NULL DEFAULT 1,
    zeus_memory_enabled INTEGER NOT NULL DEFAULT 1,
    zeus_memory_team_scope INTEGER NOT NULL DEFAULT 0,
    zeus_memory_project_scope INTEGER NOT NULL DEFAULT 1,
    zeus_memory_retention_days INTEGER NOT NULL DEFAULT 90,
    zeus_air_gap_llm INTEGER NOT NULL DEFAULT 0
);

-- ============================================================
-- users
-- ============================================================
CREATE TABLE IF NOT EXISTS users (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'viewer',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- enrollment_tokens
-- ============================================================
CREATE TABLE IF NOT EXISTS enrollment_tokens (
    token TEXT PRIMARY KEY,
    cluster_id TEXT REFERENCES clusters(id),
    expires_at TEXT,
    used_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- baremetal_servers (must precede hosts due to FK)
-- ============================================================
CREATE TABLE IF NOT EXISTS baremetal_servers (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    hostname TEXT NOT NULL,
    bmc_address TEXT NOT NULL DEFAULT '',
    bmc_type TEXT NOT NULL DEFAULT 'redfish',
    state TEXT NOT NULL DEFAULT 'discovered',
    cpu_cores INTEGER NOT NULL DEFAULT 0,
    memory_mib INTEGER NOT NULL DEFAULT 0,
    tags TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 024
    firewall_profile TEXT NOT NULL DEFAULT 'BareMetalBmc',
    firewall_enabled INTEGER NOT NULL DEFAULT 1,
    bmc_vlan TEXT NOT NULL DEFAULT '',
    pxe_vlan TEXT NOT NULL DEFAULT '',
    posture_json TEXT,
    last_exposure_scan_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_baremetal_servers_hostname ON baremetal_servers(hostname);

-- ============================================================
-- hosts
-- ============================================================
CREATE TABLE IF NOT EXISTS hosts (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT REFERENCES clusters(id),
    hostname TEXT NOT NULL,
    address TEXT NOT NULL DEFAULT '',
    agent_version TEXT NOT NULL DEFAULT '',
    libvirt_uri TEXT NOT NULL DEFAULT 'qemu:///system',
    agent_grpc_addr TEXT NOT NULL DEFAULT '127.0.0.1:50051',
    state TEXT NOT NULL DEFAULT 'unknown',
    maintenance_mode INTEGER NOT NULL DEFAULT 0,
    last_heartbeat_at TEXT,
    cpu_percent REAL NOT NULL DEFAULT 0,
    memory_used_mib INTEGER NOT NULL DEFAULT 0,
    memory_total_mib INTEGER NOT NULL DEFAULT 0,
    vm_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 002
    agent_console_addr TEXT NOT NULL DEFAULT '127.0.0.1:50052',
    -- 004
    cpu_model TEXT NOT NULL DEFAULT '',
    libvirt_version TEXT NOT NULL DEFAULT '',
    qemu_version TEXT NOT NULL DEFAULT '',
    fenced INTEGER NOT NULL DEFAULT 0,
    -- 005
    notes TEXT NOT NULL DEFAULT '',
    -- 007
    tags TEXT NOT NULL DEFAULT '[]',
    -- 008
    fence_method TEXT NOT NULL DEFAULT 'shell',
    ipmi_address TEXT NOT NULL DEFAULT '',
    ipmi_username TEXT NOT NULL DEFAULT '',
    ipmi_password TEXT NOT NULL DEFAULT '',
    -- 010
    validation_status TEXT NOT NULL DEFAULT 'pending',
    validation_report TEXT NOT NULL DEFAULT '[]',
    -- 024
    baremetal_origin_id TEXT REFERENCES baremetal_servers(id),
    -- 034
    site TEXT NOT NULL DEFAULT '',
    rack TEXT NOT NULL DEFAULT '',
    rack_u INTEGER,
    -- 043
    guacamole_base_url TEXT NOT NULL DEFAULT '',
    guacamole_json_secret_hex TEXT NOT NULL DEFAULT '',
    UNIQUE (cluster_id, hostname)
);

CREATE INDEX IF NOT EXISTS idx_hosts_validation_status ON hosts(validation_status);
CREATE INDEX IF NOT EXISTS idx_hosts_site_rack ON hosts(site, rack);

-- ============================================================
-- vms
-- ============================================================
CREATE TABLE IF NOT EXISTS vms (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT REFERENCES clusters(id),
    host_id TEXT REFERENCES hosts(id),
    name TEXT NOT NULL,
    project TEXT,
    spec_json TEXT NOT NULL DEFAULT '{}',
    desired_state TEXT NOT NULL DEFAULT 'running',
    observed_state TEXT NOT NULL DEFAULT 'unknown',
    uuid TEXT,
    vcpus INTEGER NOT NULL DEFAULT 1,
    memory_mib INTEGER NOT NULL DEFAULT 1024,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 003
    ha_recovery_count INTEGER NOT NULL DEFAULT 0,
    -- 007
    tags TEXT NOT NULL DEFAULT '[]',
    -- 010
    lifecycle_phase TEXT NOT NULL DEFAULT 'idle',
    last_error TEXT NOT NULL DEFAULT '',
    managed INTEGER NOT NULL DEFAULT 1,
    -- 013
    guest_tools_status TEXT NOT NULL DEFAULT 'unknown',
    guest_ip TEXT,
    guest_hostname TEXT,
    os_family TEXT,
    -- 035
    inventory_source TEXT NOT NULL DEFAULT 'libvirt',
    last_seen_at TEXT,
    k8s_namespace TEXT,
    k8s_uid TEXT
);

CREATE INDEX IF NOT EXISTS idx_vms_host ON vms(host_id);
CREATE INDEX IF NOT EXISTS idx_vms_lifecycle_phase ON vms(lifecycle_phase);
CREATE INDEX IF NOT EXISTS idx_vms_managed ON vms(managed);
CREATE INDEX IF NOT EXISTS idx_vms_guest_tools ON vms(guest_tools_status);
CREATE INDEX IF NOT EXISTS idx_vms_inventory_source ON vms(inventory_source);
CREATE INDEX IF NOT EXISTS idx_vms_observed_state ON vms(observed_state);
CREATE INDEX IF NOT EXISTS idx_vms_host_source ON vms(host_id, inventory_source);
-- Partial unique indexes are supported in SQLite
CREATE UNIQUE INDEX IF NOT EXISTS idx_vms_libvirt_name
    ON vms(cluster_id, name)
    WHERE inventory_source = 'libvirt';
CREATE UNIQUE INDEX IF NOT EXISTS idx_vms_kubevirt_name
    ON vms(cluster_id, k8s_namespace, name)
    WHERE inventory_source = 'kubevirt';

-- ============================================================
-- vm_disks
-- ============================================================
CREATE TABLE IF NOT EXISTS vm_disks (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    size_gib INTEGER NOT NULL,
    storage_class TEXT NOT NULL DEFAULT 'silver',
    path TEXT
);

-- ============================================================
-- templates
-- ============================================================
CREATE TABLE IF NOT EXISTS templates (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    source_disk TEXT NOT NULL,
    cloud_init INTEGER NOT NULL DEFAULT 0,
    os_family TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 014
    category TEXT NOT NULL DEFAULT 'Linux',
    description TEXT NOT NULL DEFAULT '',
    featured INTEGER NOT NULL DEFAULT 0,
    marketplace INTEGER NOT NULL DEFAULT 1,
    icon TEXT,
    -- 021
    firewall_profile TEXT,
    -- 040
    workload TEXT NOT NULL DEFAULT '',
    approval_status TEXT NOT NULL DEFAULT 'approved',
    git_ref TEXT NOT NULL DEFAULT '',
    daemon_json_path TEXT NOT NULL DEFAULT '',
    project TEXT NOT NULL DEFAULT '',
    UNIQUE (name, version)
);

CREATE INDEX IF NOT EXISTS idx_templates_marketplace ON templates(marketplace, featured);
CREATE INDEX IF NOT EXISTS idx_templates_workload ON templates(workload);
CREATE INDEX IF NOT EXISTS idx_templates_approval ON templates(approval_status);

-- ============================================================
-- tasks
-- ============================================================
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    operation TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    progress INTEGER NOT NULL DEFAULT 0,
    resource_type TEXT,
    resource_id TEXT,
    host_id TEXT REFERENCES hosts(id),
    payload TEXT NOT NULL DEFAULT '{}',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_tasks_operation ON tasks(operation);

-- ============================================================
-- task_steps
-- ============================================================
CREATE TABLE IF NOT EXISTS task_steps (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- events
-- ============================================================
CREATE TABLE IF NOT EXISTS events (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    kind TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    message TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);

-- ============================================================
-- audit_logs
-- ============================================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    detail TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_logs(actor);

-- ============================================================
-- ha_policies
-- ============================================================
CREATE TABLE IF NOT EXISTS ha_policies (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0,
    restart_attempts INTEGER NOT NULL DEFAULT 3,
    restart_priority TEXT NOT NULL DEFAULT 'medium',
    anti_affinity INTEGER NOT NULL DEFAULT 0,
    fence_on_failure INTEGER NOT NULL DEFAULT 0
);

-- ============================================================
-- migration_jobs
-- ============================================================
CREATE TABLE IF NOT EXISTS migration_jobs (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id),
    source_host_id TEXT NOT NULL REFERENCES hosts(id),
    dest_host_id TEXT NOT NULL REFERENCES hosts(id),
    live INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'pending',
    precheck TEXT NOT NULL DEFAULT '{}',
    progress INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- maintenance_windows
-- ============================================================
CREATE TABLE IF NOT EXISTS maintenance_windows (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    host_id TEXT NOT NULL REFERENCES hosts(id),
    action TEXT NOT NULL DEFAULT 'enter',
    evacuate INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- storage_pools  (tier_id FK added after storage_tiers is defined)
-- ============================================================
CREATE TABLE IF NOT EXISTS storage_pools (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT REFERENCES clusters(id),
    name TEXT NOT NULL,
    storage_class TEXT NOT NULL DEFAULT 'silver',
    backend TEXT NOT NULL DEFAULT 'directory',
    path TEXT,
    capacity_gib INTEGER NOT NULL DEFAULT 0,
    used_gib INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    tier_id TEXT,  -- FK to storage_tiers (enforced via idx after that table exists)
    UNIQUE (cluster_id, name)
);

CREATE INDEX IF NOT EXISTS idx_storage_pools_tier ON storage_pools(tier_id);

-- ============================================================
-- networks  (segment_id FK added after network_segments is defined)
-- ============================================================
CREATE TABLE IF NOT EXISTS networks (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT REFERENCES clusters(id),
    name TEXT NOT NULL,
    backend TEXT NOT NULL DEFAULT 'linux-bridge',
    vlan_id INTEGER,
    bridge TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    segment_id TEXT,  -- FK to network_segments (enforced via idx after that table exists)
    UNIQUE (cluster_id, name)
);

CREATE INDEX IF NOT EXISTS idx_networks_segment ON networks(segment_id);

-- ============================================================
-- ha_events
-- ============================================================
CREATE TABLE IF NOT EXISTS ha_events (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT REFERENCES vms(id) ON DELETE SET NULL,
    host_id TEXT REFERENCES hosts(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ha_events_created ON ha_events(created_at DESC);

-- ============================================================
-- placement_recommendations
-- ============================================================
CREATE TABLE IF NOT EXISTS placement_recommendations (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    from_host_id TEXT NOT NULL REFERENCES hosts(id),
    to_host_id TEXT NOT NULL REFERENCES hosts(id),
    reason TEXT NOT NULL,
    score REAL NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_placement_open ON placement_recommendations(status) WHERE status = 'open';

-- ============================================================
-- fence_events
-- ============================================================
CREATE TABLE IF NOT EXISTS fence_events (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    host_id TEXT NOT NULL REFERENCES hosts(id),
    action TEXT NOT NULL,
    command TEXT,
    success INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- snapshot_records
-- ============================================================
CREATE TABLE IF NOT EXISTS snapshot_records (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 006
    snapshot_path TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_snapshot_vm ON snapshot_records(vm_id);

-- ============================================================
-- backup_records
-- ============================================================
CREATE TABLE IF NOT EXISTS backup_records (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    backup_type TEXT NOT NULL DEFAULT 'full',
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 006
    backup_path TEXT NOT NULL DEFAULT '',
    -- 007
    restore_status TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_backup_vm ON backup_records(vm_id);

-- ============================================================
-- api_keys
-- ============================================================
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'operator',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

-- ============================================================
-- webhooks
-- ============================================================
CREATE TABLE IF NOT EXISTS webhooks (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    url TEXT NOT NULL,
    events TEXT NOT NULL DEFAULT '{}',
    secret TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- maintenance_schedules
-- ============================================================
CREATE TABLE IF NOT EXISTS maintenance_schedules (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    action TEXT NOT NULL DEFAULT 'enter',
    evacuate INTEGER NOT NULL DEFAULT 1,
    run_at TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_maintenance_sched_run ON maintenance_schedules(run_at) WHERE status = 'pending';

-- ============================================================
-- notification_outbox
-- ============================================================
CREATE TABLE IF NOT EXISTS notification_outbox (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    kind TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    delivered INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 007
    delivered_at TEXT
);

-- ============================================================
-- oidc_states
-- ============================================================
CREATE TABLE IF NOT EXISTS oidc_states (
    state TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_oidc_states_created ON oidc_states(created_at);

-- ============================================================
-- controller_leadership
-- ============================================================
CREATE TABLE IF NOT EXISTS controller_leadership (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    holder_id TEXT NOT NULL DEFAULT '',
    lease_until TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO controller_leadership (id, holder_id, lease_until)
VALUES (1, '', CURRENT_TIMESTAMP)
ON CONFLICT (id) DO NOTHING;

-- ============================================================
-- webhook_deliveries
-- ============================================================
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    webhook_id TEXT,
    url TEXT NOT NULL,
    secret TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL,
    event_kind TEXT NOT NULL DEFAULT '',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    next_retry_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'pending',
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_pending
    ON webhook_deliveries(next_retry_at) WHERE status = 'pending';

-- ============================================================
-- policy_rules
-- ============================================================
CREATE TABLE IF NOT EXISTS policy_rules (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    enabled INTEGER NOT NULL DEFAULT 1,
    rule_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO policy_rules (id, name, rule_json) VALUES
    (X'00000000000040008000000000000001', 'production-ha-required',
     '{"when":{"tags_contains":"production"},"require":{"ha_enabled":true}}')
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- project_quotas
-- ============================================================
CREATE TABLE IF NOT EXISTS project_quotas (
    project TEXT PRIMARY KEY,
    max_vms INTEGER NOT NULL DEFAULT 0,
    max_vcpu INTEGER NOT NULL DEFAULT 0,
    max_memory_mib INTEGER NOT NULL DEFAULT 0,
    max_storage_gib INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- backup_targets
-- ============================================================
CREATE TABLE IF NOT EXISTS backup_targets (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL DEFAULT 'local',
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- vm_metrics
-- ============================================================
CREATE TABLE IF NOT EXISTS vm_metrics (
    vm_id TEXT PRIMARY KEY REFERENCES vms(id) ON DELETE CASCADE,
    cpu_percent REAL NOT NULL DEFAULT 0,
    memory_used_mib INTEGER NOT NULL DEFAULT 0,
    disk_read_iops INTEGER NOT NULL DEFAULT 0,
    disk_write_iops INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- content_images
-- ============================================================
CREATE TABLE IF NOT EXISTS content_images (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'iso',
    path TEXT NOT NULL,
    size_gib INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'available',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 014
    category TEXT NOT NULL DEFAULT 'Custom Appliances',
    description TEXT NOT NULL DEFAULT '',
    submitted_by TEXT,
    approved_by TEXT,
    approved_at TEXT,
    rejected_reason TEXT,
    -- 015
    checksum TEXT,
    UNIQUE(cluster_id, name)
);

CREATE INDEX IF NOT EXISTS idx_content_images_kind ON content_images(kind);
CREATE INDEX IF NOT EXISTS idx_content_images_status ON content_images(status);

-- ============================================================
-- network_reservations  (pool_id FK added after network_ipam_pools is defined)
-- ============================================================
CREATE TABLE IF NOT EXISTS network_reservations (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    network_id TEXT NOT NULL REFERENCES networks(id) ON DELETE CASCADE,
    vm_id TEXT REFERENCES vms(id) ON DELETE SET NULL,
    mac_address TEXT,
    ip_address TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    pool_id TEXT,  -- FK to network_ipam_pools (defined after that table)
    hostname TEXT
);

-- ============================================================
-- application_groups
-- ============================================================
CREATE TABLE IF NOT EXISTS application_groups (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(cluster_id, name)
);

-- ============================================================
-- application_group_vms
-- ============================================================
CREATE TABLE IF NOT EXISTS application_group_vms (
    group_id TEXT NOT NULL REFERENCES application_groups(id) ON DELETE CASCADE,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, vm_id)
);

-- ============================================================
-- blueprints
-- ============================================================
CREATE TABLE IF NOT EXISTS blueprints (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    cluster_id TEXT NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    actions TEXT NOT NULL DEFAULT '[]',
    vm_ids TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (cluster_id, name)
);

CREATE INDEX IF NOT EXISTS idx_blueprints_cluster ON blueprints(cluster_id);

-- ============================================================
-- firewall_profiles
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_profiles (
    id BLOB NOT NULL PRIMARY KEY DEFAULT (randomblob(16)) CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    spec_json TEXT NOT NULL DEFAULT '{}',
    builtin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO firewall_profiles (name, display_name, spec_json, builtin) VALUES
    ('Public', 'Public', '{"default_inbound":"deny"}', 1),
    ('Private', 'Private', '{"default_inbound":"allow"}', 1),
    ('ProductionServer', 'Production Server', '{"default_inbound":"deny"}', 1),
    ('DatabaseServer', 'Database Server', '{"default_inbound":"deny"}', 1),
    ('WebServer', 'Web Server', '{"default_inbound":"deny"}', 1),
    ('KubernetesNode', 'Kubernetes Node', '{"default_inbound":"deny"}', 1),
    ('StorageNode', 'Storage Node', '{"default_inbound":"deny"}', 1),
    ('ManagementNode', 'Management Node', '{"default_inbound":"deny"}', 1),
    ('DevelopmentVm', 'Development VM', '{"default_inbound":"allow"}', 1),
    ('LockedDown', 'Locked Down', '{"default_inbound":"deny","default_outbound":"deny"}', 1),
    ('EmergencyIsolation', 'Emergency Isolation', '{"default_inbound":"deny","default_outbound":"deny","stealth":"emergency"}', 1)
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- firewall_posture_snapshots
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_posture_snapshots (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    checksum TEXT NOT NULL,
    posture_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fw_posture_target ON firewall_posture_snapshots(target_kind, target_id, captured_at DESC);

-- ============================================================
-- firewall_checkpoints
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_checkpoints (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT 'rollback',
    adapter_state TEXT NOT NULL DEFAULT '{}',
    created_by TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- firewall_temporary_rules
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_temporary_rules (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    source_cidr TEXT NOT NULL,
    dest_port INTEGER NOT NULL,
    protocol TEXT NOT NULL DEFAULT 'tcp',
    reason TEXT NOT NULL,
    owner TEXT,
    approval_id TEXT,
    expires_at TEXT NOT NULL,
    applied INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fw_temp_expiry ON firewall_temporary_rules(expires_at) WHERE applied = 1;

-- ============================================================
-- firewall_timeline
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_timeline (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    target_kind TEXT NOT NULL,
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}',
    actor TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fw_timeline_target ON firewall_timeline(target_kind, target_id, created_at DESC);

-- ============================================================
-- firewall_policies
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_policies (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    spec_yaml TEXT NOT NULL,
    workspace_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- firewall_approvals
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_approvals (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    target_kind TEXT NOT NULL DEFAULT 'host',
    target_id TEXT NOT NULL,
    profile TEXT,
    plan_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL,
    reviewed_by TEXT,
    review_note TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    reviewed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_fw_approvals_status ON firewall_approvals(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_fw_approvals_target ON firewall_approvals(target_kind, target_id);

-- ============================================================
-- firewall_policy_sync_log
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_policy_sync_log (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    direction TEXT NOT NULL,
    policy_count INTEGER NOT NULL DEFAULT 0,
    actor TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- firewall_k8s_apply_log
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_k8s_apply_log (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    namespace TEXT NOT NULL,
    profile TEXT NOT NULL,
    backend TEXT NOT NULL,
    actor TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- firewall_cloud_snapshots
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_cloud_snapshots (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    provider TEXT NOT NULL,
    summary TEXT NOT NULL,
    inventory_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fw_cloud_provider ON firewall_cloud_snapshots(provider, captured_at DESC);

-- ============================================================
-- firewall_connectivity_runs
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_connectivity_runs (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    target_id TEXT NOT NULL,
    profile TEXT NOT NULL,
    matrix_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- firewall_policy_reconcile_log
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_policy_reconcile_log (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    policies_synced INTEGER NOT NULL DEFAULT 0,
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- firewall_sites
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_sites (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    region TEXT NOT NULL DEFAULT 'local',
    role TEXT NOT NULL DEFAULT 'primary',
    gitops_namespace TEXT NOT NULL DEFAULT 'default',
    lockdown_enabled INTEGER NOT NULL DEFAULT 0,
    geo_fence TEXT,
    dr_pair TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO firewall_sites (id, name, region, role, gitops_namespace, dr_pair)
VALUES (X'10000000000040008000000000000001', 'primary-local', 'local', 'primary', 'site-primary', 'dr-replica')
ON CONFLICT (name) DO NOTHING;

INSERT INTO firewall_sites (id, name, region, role, gitops_namespace, dr_pair, lockdown_enabled)
VALUES (X'20000000000040008000000000000001', 'dr-replica', 'dr', 'replica', 'site-dr', 'primary-local', 0)
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- firewall_site_policies
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_site_policies (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    site_id TEXT NOT NULL REFERENCES firewall_sites(id) ON DELETE CASCADE,
    policy_name TEXT NOT NULL,
    profile TEXT NOT NULL DEFAULT 'ProductionServer',
    spec_yaml TEXT NOT NULL DEFAULT '',
    geo_fence TEXT,
    dr_pair TEXT,
    stretch_deny INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (site_id, policy_name)
);

CREATE INDEX IF NOT EXISTS idx_fw_site_policies_site ON firewall_site_policies(site_id);

-- ============================================================
-- firewall_site_drift
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_site_drift (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    site_id TEXT NOT NULL REFERENCES firewall_sites(id) ON DELETE CASCADE,
    peer_site_id TEXT REFERENCES firewall_sites(id) ON DELETE SET NULL,
    drift_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fw_site_drift_site ON firewall_site_drift(site_id, captured_at DESC);

-- ============================================================
-- firewall_site_timeline
-- ============================================================
CREATE TABLE IF NOT EXISTS firewall_site_timeline (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    site_id TEXT NOT NULL REFERENCES firewall_sites(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    detail_json TEXT NOT NULL DEFAULT '{}',
    actor TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- network_segments
-- ============================================================
CREATE TABLE IF NOT EXISTS network_segments (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    tier TEXT NOT NULL DEFAULT 'tier1',
    cidr TEXT NOT NULL,
    east_west_default TEXT NOT NULL DEFAULT 'allow',
    firewall_profile TEXT,
    gitops_namespace TEXT NOT NULL DEFAULT 'network-segments',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- network_ipam_pools
-- ============================================================
CREATE TABLE IF NOT EXISTS network_ipam_pools (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    segment_id TEXT NOT NULL REFERENCES network_segments(id) ON DELETE CASCADE,
    cidr TEXT NOT NULL,
    gateway TEXT,
    dns_json TEXT NOT NULL DEFAULT '[]',
    next_offset INTEGER NOT NULL DEFAULT 2,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ipam_pools_segment ON network_ipam_pools(segment_id);

INSERT INTO network_segments (id, name, tier, cidr, east_west_default, firewall_profile, gitops_namespace)
VALUES
    (X'a1000000000040008000000000000001', 'prod-tier1', 'tier1', '10.10.0.0/16', 'allow', 'ProductionServer', 'prod-segments'),
    (X'a1000000000040008000000000000002', 'dmz-tier0', 'tier0', '172.16.0.0/24', 'deny', 'WebServer', 'dmz-segments')
ON CONFLICT (name) DO NOTHING;

INSERT INTO network_ipam_pools (id, segment_id, cidr, gateway, dns_json, next_offset)
VALUES
    (X'b1000000000040008000000000000001', X'a1000000000040008000000000000001', '10.10.0.0/16', '10.10.0.1', '["10.10.0.1"]', 10),
    (X'b1000000000040008000000000000002', X'a1000000000040008000000000000002', '172.16.0.0/24', '172.16.0.1', '["172.16.0.1"]', 10)
ON CONFLICT (id) DO NOTHING;

-- ============================================================
-- platform_plugins
-- ============================================================
CREATE TABLE IF NOT EXISTS platform_plugins (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'integration',
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    author TEXT NOT NULL DEFAULT 'Zyvor',
    featured INTEGER NOT NULL DEFAULT 0,
    installed INTEGER NOT NULL DEFAULT 0,
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_platform_plugins_category ON platform_plugins(category, featured);

INSERT INTO platform_plugins (id, slug, name, category, description, version, author, featured, installed)
VALUES
    (X'c2000000000040008000000000000001', 'guestkit', 'GuestKit', 'automation', 'Guest health checks, job runner, and in-VM automation bridge.', '1.0.0', 'Zyvor', 1, 0),
    (X'c2000000000040008000000000000002', 'packetwolf', 'PacketWolf', 'observability', 'Flow capture, anomaly hints, and firewall activity correlation.', '1.0.0', 'Zyvor', 1, 0),
    (X'c2000000000040008000000000000003', 'hypersdk', 'HyperSDK', 'migration', 'P2V migration assistant and Windows VM discovery.', '1.0.0', 'Zyvor', 1, 0),
    (X'c2000000000040008000000000000004', 'zeus-firewall', 'Zeus Firewall', 'security', 'Fleet machine shield, profiles, and connectivity simulation.', '1.0.0', 'Zyvor', 1, 1),
    (X'c2000000000040008000000000000005', 'kubevirt-bridge', 'KubeVirt Bridge', 'kubernetes', 'Export libvirt VMs and qcow2 bundles for Kubernetes.', '1.0.0', 'Zyvor', 0, 0),
    (X'c2000000000040008000000000000006', 'network-overlay', 'Network Overlay', 'networking', 'NSX-class segments, IPAM pools, and micro-segmentation stubs.', '1.0.0', 'Zyvor', 0, 1),
    (X'c2000000000040008000000000000010', 'kasm-workspaces', 'Kasm Workspaces', 'console', 'Disposable browser and isolated desktop labs (Marketplace workload — not core ConsoleHub).', '1.0.0', 'Kasm', 0, 0),
    (X'c2000000000040008000000000000011', 'rustdesk', 'RustDesk', 'console', 'TeamViewer-style remote support sessions via Marketplace plugin.', '1.0.0', 'RustDesk', 0, 0),
    (X'c2000000000040008000000000000012', 'meshcentral', 'MeshCentral', 'console', 'Remote management and support gateway as optional Marketplace plugin.', '1.0.0', 'MeshCentral', 0, 0)
ON CONFLICT (slug) DO NOTHING;

-- ============================================================
-- storage_tiers
-- ============================================================
CREATE TABLE IF NOT EXISTS storage_tiers (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    tier_class TEXT NOT NULL DEFAULT 'silver',
    iops_tier TEXT NOT NULL DEFAULT 'standard',
    replication TEXT NOT NULL DEFAULT 'local',
    snapshot_retention_days INTEGER NOT NULL DEFAULT 7,
    backup_rpo_hours INTEGER NOT NULL DEFAULT 24,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO storage_tiers (id, name, tier_class, iops_tier, replication, snapshot_retention_days, backup_rpo_hours, description)
VALUES
    (X'd1000000000040008000000000000001', 'gold-performance', 'gold', 'nvme', 'sync-mirror', 30, 4, 'Low-latency NVMe tier with synchronous mirror stub'),
    (X'd1000000000040008000000000000002', 'silver-standard', 'silver', 'standard', 'local', 14, 24, 'Default production datastore tier'),
    (X'd1000000000040008000000000000003', 'bronze-archive', 'bronze', 'hdd', 'local', 7, 72, 'Capacity-optimized cold tier')
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- storage_backup_sla
-- ============================================================
CREATE TABLE IF NOT EXISTS storage_backup_sla (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    pool_id TEXT NOT NULL REFERENCES storage_pools(id) ON DELETE CASCADE,
    rpo_hours INTEGER NOT NULL DEFAULT 24,
    rto_hours INTEGER NOT NULL DEFAULT 4,
    retention_days INTEGER NOT NULL DEFAULT 30,
    last_backup_at TEXT,
    compliance_grade TEXT NOT NULL DEFAULT 'B',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (pool_id)
);

-- ============================================================
-- vault_providers
-- ============================================================
CREATE TABLE IF NOT EXISTS vault_providers (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    provider_type TEXT NOT NULL DEFAULT 'hashicorp',
    address TEXT NOT NULL DEFAULT '',
    namespace TEXT NOT NULL DEFAULT 'machina',
    status TEXT NOT NULL DEFAULT 'disconnected',
    last_sync_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO vault_providers (id, name, provider_type, address, namespace, status)
VALUES
    (X'e1000000000040008000000000000001', 'local-config', 'file', '', 'machina', 'active'),
    (X'e1000000000040008000000000000002', 'vault-stub', 'hashicorp', 'https://vault.example:8200', 'machina', 'disconnected')
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- mfa_policies
-- ============================================================
CREATE TABLE IF NOT EXISTS mfa_policies (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    role_name TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL DEFAULT 'webauthn',
    required INTEGER NOT NULL DEFAULT 0,
    grace_days INTEGER NOT NULL DEFAULT 7,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO mfa_policies (id, role_name, method, required, grace_days)
VALUES
    (X'e2000000000040008000000000000001', 'admin', 'webauthn', 0, 7),
    (X'e2000000000040008000000000000002', 'operator', 'totp', 0, 14),
    (X'e2000000000040008000000000000003', 'viewer', 'totp', 0, 30)
ON CONFLICT (role_name) DO NOTHING;

-- ============================================================
-- air_gap_bundles
-- ============================================================
CREATE TABLE IF NOT EXISTS air_gap_bundles (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    checksum TEXT NOT NULL DEFAULT '',
    manifest_json TEXT NOT NULL DEFAULT '{}',
    size_bytes INTEGER NOT NULL DEFAULT 0,
    exported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_air_gap_bundles_exported ON air_gap_bundles(exported_at DESC);

-- ============================================================
-- host_lldp_cache
-- ============================================================
CREATE TABLE IF NOT EXISTS host_lldp_cache (
    host_id TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT '',
    neighbors_json TEXT NOT NULL DEFAULT '[]',
    summary TEXT NOT NULL DEFAULT '',
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_host_lldp_cache_fetched ON host_lldp_cache(fetched_at DESC);

-- ============================================================
-- ops_runbook_catalog
-- ============================================================
CREATE TABLE IF NOT EXISTS ops_runbook_catalog (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    incident TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'incident',
    severity TEXT NOT NULL DEFAULT 'medium',
    auto_trigger TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- 032
    last_triggered_at TEXT
);

INSERT INTO ops_runbook_catalog (id, incident, title, category, severity, auto_trigger)
VALUES
    (X'f1000000000040008000000000000001', 'host_offline', 'Host offline recovery', 'incident', 'high', 'host.state=offline'),
    (X'f1000000000040008000000000000002', 'backup_failed', 'Backup failure triage', 'incident', 'medium', 'task.failed:backup'),
    (X'f1000000000040008000000000000003', 'migration_failed', 'Migration failure triage', 'incident', 'medium', 'task.failed:migrate'),
    (X'f1000000000040008000000000000004', 'firewall_drift', 'Firewall drift remediation', 'compliance', 'high', 'zeus.drift_detected'),
    (X'f1000000000040008000000000000005', 'storage_full', 'Storage pool capacity', 'capacity', 'critical', 'storage.used_pct>85')
ON CONFLICT (incident) DO NOTHING;

-- ============================================================
-- ops_runbook_executions
-- ============================================================
CREATE TABLE IF NOT EXISTS ops_runbook_executions (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    incident TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'completed',
    steps_json TEXT NOT NULL DEFAULT '[]',
    actor TEXT,
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ops_runbook_exec_created ON ops_runbook_executions(created_at DESC);

-- ============================================================
-- ops_showback_snapshots
-- ============================================================
CREATE TABLE IF NOT EXISTS ops_showback_snapshots (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    project_name TEXT NOT NULL,
    cost_usd REAL NOT NULL DEFAULT 0,
    compliance_grade TEXT NOT NULL DEFAULT 'B',
    vm_count INTEGER NOT NULL DEFAULT 0,
    captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ops_showback_project ON ops_showback_snapshots(project_name, captured_at DESC);

-- ============================================================
-- slo_policies
-- ============================================================
CREATE TABLE IF NOT EXISTS slo_policies (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    target TEXT NOT NULL,
    objective_pct REAL NOT NULL DEFAULT 99.9,
    window_hours INTEGER NOT NULL DEFAULT 720,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO slo_policies (id, name, target, objective_pct, window_hours, description)
VALUES
    (X'a1000000000040008000000000000001', 'api-availability', 'controller /api/v1/*', 99.5, 720, 'HTTP 2xx/3xx rate for platform API'),
    (X'a1000000000040008000000000000002', 'task-success', 'platform tasks', 98.0, 168, 'Completed vs failed task ratio'),
    (X'a1000000000040008000000000000003', 'host-availability', 'online hosts', 99.0, 720, 'Hosts reporting online vs registered')
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- api_trace_spans
-- ============================================================
CREATE TABLE IF NOT EXISTS api_trace_spans (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_api_trace_recorded ON api_trace_spans(recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_trace_path ON api_trace_spans(path, recorded_at DESC);

-- ============================================================
-- vault_sync_runs
-- ============================================================
CREATE TABLE IF NOT EXISTS vault_sync_runs (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    provider_id TEXT NOT NULL REFERENCES vault_providers(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_vault_sync_runs_provider ON vault_sync_runs(provider_id, recorded_at DESC);

-- ============================================================
-- mfa_enrollments
-- ============================================================
CREATE TABLE IF NOT EXISTS mfa_enrollments (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    username TEXT NOT NULL UNIQUE,
    method TEXT NOT NULL DEFAULT 'webauthn',
    enrolled_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- fips_crypto_profiles
-- ============================================================
CREATE TABLE IF NOT EXISTS fips_crypto_profiles (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    tls_min_version TEXT NOT NULL DEFAULT '1.2',
    fips_mode TEXT NOT NULL DEFAULT 'disabled',
    cipher_suites TEXT NOT NULL DEFAULT 'system-default',
    notes TEXT NOT NULL DEFAULT ''
);

INSERT INTO fips_crypto_profiles (id, name, tls_min_version, fips_mode, cipher_suites, notes)
VALUES
    (X'f1000000000040008000000000000001', 'platform-default', '1.2', 'disabled', 'TLS_AES_128_GCM_SHA256,TLS_AES_256_GCM_SHA384', 'Controller TLS via system OpenSSL — FIPS module not selected'),
    (X'f1000000000040008000000000000002', 'fips-ready', '1.2', 'required', 'TLS_AES_256_GCM_SHA384', 'Target profile for FIPS 140-3 validated module rollout')
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- tenant_isolation_policies
-- ============================================================
CREATE TABLE IF NOT EXISTS tenant_isolation_policies (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    project_name TEXT NOT NULL UNIQUE,
    network_isolation TEXT NOT NULL DEFAULT 'shared',
    max_vms INTEGER NOT NULL DEFAULT 0,
    max_storage_gib INTEGER NOT NULL DEFAULT 0,
    enforce_quotas INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO tenant_isolation_policies (id, project_name, network_isolation, max_vms, max_storage_gib, enforce_quotas)
VALUES
    (X'10000000000040008000000000000001', 'default', 'shared', 0, 0, 0),
    (X'10000000000040008000000000000002', 'production', 'segmented', 50, 10240, 1)
ON CONFLICT (project_name) DO NOTHING;

-- ============================================================
-- ai_providers
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_providers (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'openai',
    base_url TEXT NOT NULL DEFAULT '',
    org_id TEXT NOT NULL DEFAULT '',
    deployment_name TEXT NOT NULL DEFAULT '',
    api_key_encrypted TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- ai_models
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_models (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    provider_id TEXT NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    context_window INTEGER NOT NULL DEFAULT 128000,
    enabled INTEGER NOT NULL DEFAULT 1,
    UNIQUE (provider_id, model_id)
);

-- ============================================================
-- ai_user_preferences
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_user_preferences (
    user_id TEXT PRIMARY KEY,
    default_provider_id TEXT REFERENCES ai_providers(id) ON DELETE SET NULL,
    default_model_id TEXT REFERENCES ai_models(id) ON DELETE SET NULL,
    default_agent TEXT NOT NULL DEFAULT 'auto',
    memory_enabled INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- ai_routing_rules
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_routing_rules (
    id BLOB NOT NULL PRIMARY KEY DEFAULT (randomblob(16)) CHECK(length(id) = 16),
    task_class TEXT NOT NULL UNIQUE,
    provider_id TEXT REFERENCES ai_providers(id) ON DELETE SET NULL,
    model_id TEXT REFERENCES ai_models(id) ON DELETE SET NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1
);

INSERT INTO ai_routing_rules (task_class, priority, enabled) VALUES
    ('infrastructure', 10, 1),
    ('code_generation', 20, 1),
    ('security_analysis', 30, 1),
    ('research', 40, 1),
    ('long_context', 50, 1),
    ('fast_local', 60, 1)
ON CONFLICT (task_class) DO NOTHING;

-- ============================================================
-- ai_prompts
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_prompts (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    scope TEXT NOT NULL DEFAULT 'personal',
    owner_id TEXT NOT NULL DEFAULT '',
    team_id TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    agent_id TEXT NOT NULL DEFAULT 'auto',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ai_prompts_owner ON ai_prompts(owner_id);

-- ============================================================
-- ai_memory_entries
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_memory_entries (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    scope TEXT NOT NULL DEFAULT 'user',
    subject_kind TEXT NOT NULL DEFAULT 'conversation',
    subject_id TEXT NOT NULL DEFAULT '',
    owner_id TEXT NOT NULL DEFAULT '',
    project_id TEXT NOT NULL DEFAULT '',
    summary TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ai_memory_owner ON ai_memory_entries(owner_id);

-- ============================================================
-- ai_conversations
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_conversations (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    user_id TEXT NOT NULL,
    project_id TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL DEFAULT 'auto',
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- ai_actions
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_actions (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    source TEXT NOT NULL DEFAULT 'zeus',
    action_type TEXT NOT NULL,
    label TEXT NOT NULL,
    review TEXT NOT NULL DEFAULT '',
    risk TEXT NOT NULL DEFAULT 'Review required',
    object_ref TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    requested_by TEXT NOT NULL DEFAULT '',
    approved_by TEXT,
    executed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ai_actions_status ON ai_actions(status);

-- ============================================================
-- ai_agent_plugins
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_agent_plugins (
    slug TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL,
    config_schema_json TEXT NOT NULL DEFAULT '{}',
    installed INTEGER NOT NULL DEFAULT 0,
    published_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO ai_agent_plugins (slug, name, description, agent_id, installed) VALUES
    ('aws-expert', 'AWS Expert', 'Cloud architecture and AWS service guidance', 'architect', 0),
    ('azure-expert', 'Azure Expert', 'Azure landing zones and NSG guidance', 'architect', 0),
    ('gcp-expert', 'GCP Expert', 'GCP networking and GKE guidance', 'architect', 0),
    ('linux-expert', 'Linux Expert', 'Host tuning and systemd diagnostics', 'sre', 0),
    ('kubernetes-expert', 'Kubernetes Expert', 'Cluster ops and workload placement', 'kubernetes', 0),
    ('terraform-expert', 'Terraform Expert', 'IaC generation and module guidance', 'architect', 0),
    ('finops-expert', 'FinOps Expert', 'Cost optimization and chargeback', 'cost', 0),
    ('security-expert', 'Security Expert', 'Threat hunting and compliance', 'security', 0)
ON CONFLICT (slug) DO NOTHING;

-- ============================================================
-- ai_incidents
-- ============================================================
CREATE TABLE IF NOT EXISTS ai_incidents (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    affected_resources TEXT NOT NULL DEFAULT '[]',
    root_cause TEXT,
    evidence_json TEXT NOT NULL DEFAULT '{}',
    window_start TEXT,
    window_end TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_ai_incidents_status ON ai_incidents(status);
CREATE INDEX IF NOT EXISTS idx_ai_incidents_created ON ai_incidents(created_at DESC);

-- ============================================================
-- soc_events
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_events (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    occurred_at TEXT NOT NULL,
    source TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'security',
    severity TEXT NOT NULL DEFAULT 'info',
    host_id TEXT,
    vm_id TEXT,
    actor TEXT,
    summary TEXT NOT NULL,
    ecs_json TEXT NOT NULL DEFAULT '{}',
    raw_ref TEXT NOT NULL DEFAULT '{}',
    dedupe_key TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_soc_events_dedupe
    ON soc_events(dedupe_key) WHERE dedupe_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_soc_events_occurred ON soc_events(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_soc_events_severity ON soc_events(severity, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_soc_events_source ON soc_events(source, occurred_at DESC);

-- ============================================================
-- soc_detection_rules
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_detection_rules (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    severity TEXT NOT NULL DEFAULT 'medium',
    query_json TEXT NOT NULL DEFAULT '{}',
    throttle_minutes INTEGER NOT NULL DEFAULT 60,
    builtin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO soc_detection_rules (id, name, description, enabled, severity, query_json, throttle_minutes, builtin)
VALUES
    (X'a1000001000140018001000000000001', 'critical_anomaly',
     'PacketWolf critical or high severity anomaly', 1, 'high',
     '{"type":"match","match":{"source":"packetwolf","severity":["critical","high"]}}', 30, 1),
    (X'a1000001000140018001000000000002', 'firewall_deny_spike',
     'Three or more firewall deny events in 15 minutes', 1, 'medium',
     '{"type":"threshold","match":{"source":"firewall","category":"firewall"},"window_minutes":15,"min_count":3}', 60, 1),
    (X'a1000001000140018001000000000003', 'brute_force_ssh',
     'Repeated failed SSH or auth audit events', 1, 'high',
     '{"type":"threshold","match":{"source":"audit","ecs.event.action":["auth.failure","login.failed"]},"window_minutes":10,"min_count":5}', 120, 1),
    (X'a1000001000140018001000000000004', 'new_admin_api_key',
     'New API key created by admin actor', 1, 'medium',
     '{"type":"match","match":{"source":"audit","ecs.event.action":["api_key.create","api_keys.create"]}}', 60, 1)
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- soc_alerts
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_alerts (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    rule_id TEXT REFERENCES soc_detection_rules(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'medium',
    status TEXT NOT NULL DEFAULT 'open',
    assigned_to TEXT,
    dedupe_key TEXT NOT NULL,
    first_seen TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    event_count INTEGER NOT NULL DEFAULT 1,
    event_ids TEXT NOT NULL DEFAULT '[]',
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_soc_alerts_dedupe_open
    ON soc_alerts(dedupe_key) WHERE status IN ('open', 'acknowledged');
CREATE INDEX IF NOT EXISTS idx_soc_alerts_status ON soc_alerts(status, last_seen DESC);

-- ============================================================
-- soc_integrations
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_integrations (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    integration_type TEXT NOT NULL,
    name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0,
    config_json TEXT NOT NULL DEFAULT '{}',
    last_success_at TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(integration_type, name)
);

INSERT INTO soc_integrations (id, integration_type, name, enabled, config_json)
VALUES
    (X'b2000002000240028002000000000001', 'splunk_hec', 'default', 0,
     '{"url":"","token":"","index":"machina","sourcetype_events":"machina:soc:ecs","sourcetype_alerts":"machina:soc:alert","host":""}'),
    (X'b2000002000240028002000000000002', 'elastic_bulk', 'default', 0,
     '{"url":"","api_key":"","index":"logs-machina.soc","pipeline":""}'),
    (X'b2000002000240028002000000000003', 'sentinel_dcr', 'default', 0,
     '{"dce_endpoint":"","dcr_immutable_id":"","stream_name":"","tenant_id":"","client_id":"","client_secret":""}'),
    (X'b2000002000240028002000000000004', 'qradar_rest', 'default', 0,
     '{"url":"","api_token":"","log_source_id":""}')
ON CONFLICT (integration_type, name) DO NOTHING;

-- ============================================================
-- soc_forwarder_cursors
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_forwarder_cursors (
    integration_id TEXT NOT NULL REFERENCES soc_integrations(id) ON DELETE CASCADE,
    cursor_kind TEXT NOT NULL DEFAULT 'events',
    last_occurred_at TEXT,
    last_event_id TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (integration_id, cursor_kind)
);

-- ============================================================
-- soc_event_exports
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_event_exports (
    integration_id TEXT NOT NULL REFERENCES soc_integrations(id) ON DELETE CASCADE,
    resource_type TEXT NOT NULL DEFAULT 'event',
    resource_id TEXT NOT NULL,
    exported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (integration_id, resource_type, resource_id)
);

-- ============================================================
-- soc_ingest_watermarks
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_ingest_watermarks (
    source TEXT PRIMARY KEY,
    last_at TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'
);

INSERT INTO soc_ingest_watermarks (source, last_at) VALUES
    ('firewall_timeline', '1970-01-01T00:00:00Z'),
    ('audit_logs', '1970-01-01T00:00:00Z'),
    ('packetwolf', '1970-01-01T00:00:00Z'),
    ('platform_events', '1970-01-01T00:00:00Z')
ON CONFLICT (source) DO NOTHING;

-- ============================================================
-- soc_saved_hunts
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_saved_hunts (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    query_text TEXT NOT NULL,
    schedule_cron TEXT,
    enabled INTEGER NOT NULL DEFAULT 0,
    last_run_at TEXT,
    created_by TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- soc_playbooks
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_playbooks (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    trigger_json TEXT NOT NULL DEFAULT '{}',
    steps_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO soc_playbooks (id, name, description, enabled, trigger_json, steps_json)
VALUES
    (X'c3000003000340038003000000000001', 'notify_on_critical',
     'Webhook notify when critical SOC alert opens', 1,
     '{"min_severity":"high","rule_names":[]}',
     '[{"type":"webhook","url_from_setting":"soc_webhook_url","body":{"alert_id":"{{alert_id}}","title":"{{title}}","severity":"{{severity}}"}}]')
ON CONFLICT (name) DO NOTHING;

-- ============================================================
-- soc_playbook_runs
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_playbook_runs (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    playbook_id TEXT NOT NULL REFERENCES soc_playbooks(id) ON DELETE CASCADE,
    alert_id TEXT REFERENCES soc_alerts(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'running',
    step_results TEXT NOT NULL DEFAULT '[]',
    error TEXT,
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_soc_playbook_runs_alert ON soc_playbook_runs(alert_id, started_at DESC);

-- ============================================================
-- soc_settings
-- ============================================================
CREATE TABLE IF NOT EXISTS soc_settings (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    webhook_url TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO soc_settings (id, webhook_url) VALUES (1, '')
ON CONFLICT (id) DO NOTHING;

-- ============================================================
-- fleet_snapshot_schedules
-- ============================================================
CREATE TABLE IF NOT EXISTS fleet_snapshot_schedules (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    name TEXT NOT NULL,
    cron_expr TEXT NOT NULL DEFAULT '0 2 * * *',
    project TEXT NOT NULL DEFAULT '',
    tag_filter TEXT NOT NULL DEFAULT '',
    disk_only INTEGER NOT NULL DEFAULT 1,
    quiesce INTEGER NOT NULL DEFAULT 0,
    retain_count INTEGER NOT NULL DEFAULT 5,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fleet_snapshot_schedules_enabled ON fleet_snapshot_schedules(enabled);

-- ============================================================
-- console_sessions
-- ============================================================
CREATE TABLE IF NOT EXISTS console_sessions (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    host_id TEXT REFERENCES hosts(id) ON DELETE SET NULL,
    actor TEXT NOT NULL DEFAULT '',
    actor_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    protocol TEXT NOT NULL,
    backend TEXT NOT NULL DEFAULT 'native',
    guac_token TEXT,
    agent_proxy_base TEXT NOT NULL DEFAULT '',
    emergency_url TEXT,
    started_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ended_at TEXT,
    expires_at TEXT NOT NULL,
    audit_id TEXT,
    recording_enabled INTEGER NOT NULL DEFAULT 0,
    recording_path TEXT,
    spectator_token TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_console_sessions_vm ON console_sessions(vm_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_console_sessions_actor ON console_sessions(actor, started_at DESC);

-- ============================================================
-- console_access_requests
-- ============================================================
CREATE TABLE IF NOT EXISTS console_access_requests (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    requester TEXT NOT NULL,
    requester_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    protocol TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    approved_by TEXT,
    approved_at TEXT,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_console_access_requests_status ON console_access_requests(status, created_at DESC);

-- ============================================================
-- packetwolf_local_sensors
-- ============================================================
CREATE TABLE IF NOT EXISTS packetwolf_local_sensors (
    host_id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'registered',
    tetragon_version TEXT NOT NULL DEFAULT '1.7.0',
    registered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_event_at TEXT,
    pending_tetragon TEXT
);

CREATE INDEX IF NOT EXISTS idx_packetwolf_local_sensors_status
    ON packetwolf_local_sensors(status);
