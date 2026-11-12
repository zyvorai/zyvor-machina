-- First-party OpenStack importer: connections, cached inventory, import jobs.
CREATE TABLE IF NOT EXISTS openstack_connections (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    auth_url TEXT NOT NULL,
    username TEXT NOT NULL DEFAULT '',
    user_domain TEXT NOT NULL DEFAULT 'Default',
    project_name TEXT NOT NULL DEFAULT '',
    project_domain TEXT NOT NULL DEFAULT 'Default',
    region TEXT NOT NULL DEFAULT 'RegionOne',
    password_enc TEXT NOT NULL DEFAULT '',
    app_cred_id TEXT NOT NULL DEFAULT '',
    app_cred_secret_enc TEXT NOT NULL DEFAULT '',
    insecure_tls INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'ready',
    last_error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS openstack_inventory (
    id TEXT NOT NULL PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES openstack_connections(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT '',
    size_bytes INTEGER,
    disk_format TEXT NOT NULL DEFAULT '',
    meta_json TEXT NOT NULL DEFAULT '{}',
    synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(connection_id, kind, remote_id)
);
CREATE INDEX IF NOT EXISTS idx_os_inventory_conn ON openstack_inventory (connection_id, kind);

CREATE TABLE IF NOT EXISTS openstack_import_jobs (
    id TEXT NOT NULL PRIMARY KEY,
    connection_id TEXT NOT NULL REFERENCES openstack_connections(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    target TEXT NOT NULL DEFAULT 'template',
    host_id TEXT,
    dest_name TEXT NOT NULL DEFAULT '',
    staged_path TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'queued',
    progress INTEGER NOT NULL DEFAULT 0,
    error TEXT NOT NULL DEFAULT '',
    template_id TEXT,
    task_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_os_import_jobs_status ON openstack_import_jobs (status);
