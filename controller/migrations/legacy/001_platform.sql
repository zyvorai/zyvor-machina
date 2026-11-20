-- Platform control plane schema (PostgreSQL)

CREATE TABLE IF NOT EXISTS clusters (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    ha_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    placement_policy TEXT NOT NULL DEFAULT 'balanced',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'viewer',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS enrollment_tokens (
    token TEXT PRIMARY KEY,
    cluster_id UUID REFERENCES clusters(id),
    expires_at TIMESTAMPTZ,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS hosts (
    id UUID PRIMARY KEY,
    cluster_id UUID REFERENCES clusters(id),
    hostname TEXT NOT NULL,
    address TEXT NOT NULL DEFAULT '',
    agent_version TEXT NOT NULL DEFAULT '',
    libvirt_uri TEXT NOT NULL DEFAULT 'qemu:///system',
    agent_grpc_addr TEXT NOT NULL DEFAULT '127.0.0.1:50051',
    state TEXT NOT NULL DEFAULT 'unknown',
    maintenance_mode BOOLEAN NOT NULL DEFAULT FALSE,
    last_heartbeat_at TIMESTAMPTZ,
    cpu_percent REAL NOT NULL DEFAULT 0,
    memory_used_mib BIGINT NOT NULL DEFAULT 0,
    memory_total_mib BIGINT NOT NULL DEFAULT 0,
    vm_count INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (cluster_id, hostname)
);

CREATE TABLE IF NOT EXISTS vms (
    id UUID PRIMARY KEY,
    cluster_id UUID REFERENCES clusters(id),
    host_id UUID REFERENCES hosts(id),
    name TEXT NOT NULL,
    project TEXT,
    spec_json JSONB NOT NULL DEFAULT '{}',
    desired_state TEXT NOT NULL DEFAULT 'running',
    observed_state TEXT NOT NULL DEFAULT 'unknown',
    uuid TEXT,
    vcpus INT NOT NULL DEFAULT 1,
    memory_mib BIGINT NOT NULL DEFAULT 1024,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (cluster_id, name)
);

CREATE TABLE IF NOT EXISTS vm_disks (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    size_gib BIGINT NOT NULL,
    storage_class TEXT NOT NULL DEFAULT 'silver',
    path TEXT
);

CREATE TABLE IF NOT EXISTS templates (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    source_disk TEXT NOT NULL,
    cloud_init BOOLEAN NOT NULL DEFAULT FALSE,
    os_family TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (name, version)
);

CREATE TABLE IF NOT EXISTS tasks (
    id UUID PRIMARY KEY,
    operation TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    progress SMALLINT NOT NULL DEFAULT 0,
    resource_type TEXT,
    resource_id UUID,
    host_id UUID REFERENCES hosts(id),
    payload JSONB NOT NULL DEFAULT '{}',
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS task_steps (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS events (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    resource_type TEXT,
    resource_id UUID,
    message TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id UUID,
    detail JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ha_policies (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    restart_attempts INT NOT NULL DEFAULT 3,
    restart_priority TEXT NOT NULL DEFAULT 'medium',
    anti_affinity BOOLEAN NOT NULL DEFAULT FALSE,
    fence_on_failure BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS migration_jobs (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id),
    source_host_id UUID NOT NULL REFERENCES hosts(id),
    dest_host_id UUID NOT NULL REFERENCES hosts(id),
    live BOOLEAN NOT NULL DEFAULT TRUE,
    status TEXT NOT NULL DEFAULT 'pending',
    precheck JSONB NOT NULL DEFAULT '{}',
    progress SMALLINT NOT NULL DEFAULT 0,
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS maintenance_windows (
    id UUID PRIMARY KEY,
    host_id UUID NOT NULL REFERENCES hosts(id),
    action TEXT NOT NULL DEFAULT 'enter',
    evacuate BOOLEAN NOT NULL DEFAULT TRUE,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_vms_host ON vms(host_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_events_created ON events(created_at DESC);
