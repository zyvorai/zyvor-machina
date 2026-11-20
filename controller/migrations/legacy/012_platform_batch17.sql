-- Batch 17-19: VM metrics snapshots, content library, network reservations

CREATE TABLE IF NOT EXISTS vm_metrics (
    vm_id UUID PRIMARY KEY REFERENCES vms(id) ON DELETE CASCADE,
    cpu_percent REAL NOT NULL DEFAULT 0,
    memory_used_mib BIGINT NOT NULL DEFAULT 0,
    disk_read_iops BIGINT NOT NULL DEFAULT 0,
    disk_write_iops BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS content_images (
    id UUID PRIMARY KEY,
    cluster_id UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'iso',
    path TEXT NOT NULL,
    size_gib BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'available',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(cluster_id, name)
);

CREATE TABLE IF NOT EXISTS network_reservations (
    id UUID PRIMARY KEY,
    network_id UUID NOT NULL REFERENCES networks(id) ON DELETE CASCADE,
    vm_id UUID REFERENCES vms(id) ON DELETE SET NULL,
    mac_address TEXT,
    ip_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_vms_managed ON vms(managed);
CREATE INDEX IF NOT EXISTS idx_content_images_kind ON content_images(kind);
