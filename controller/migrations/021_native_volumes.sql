-- Standalone, project-owned storage volumes — Phase C of the next-gen roadmap
-- (see /Users/ssahani/.claude/plans/lazy-munching-quilt.md). Distinct from `vm_disks`,
-- which is always created alongside a specific VM: a volume here can be created
-- unattached and attached to any VM later. Backed by the existing Atlas storage
-- control plane (real Ceph/RBD, engine::atlas_bridge) when ATLAS_ENABLED, falling
-- back to the local storage-pool agent RPC (api::storage) otherwise.
CREATE TABLE IF NOT EXISTS volumes (
    id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    size_gib INTEGER NOT NULL,
    volume_class TEXT NOT NULL DEFAULT 'silver',
    storage_pool_id TEXT REFERENCES storage_pools(id) ON DELETE SET NULL,
    path TEXT,
    atlas_volume_id TEXT,
    status TEXT NOT NULL DEFAULT 'creating',
    attached_vm_id TEXT REFERENCES vms(id) ON DELETE SET NULL,
    attached_device TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_volumes_project ON volumes(project_id);
CREATE INDEX IF NOT EXISTS idx_volumes_attached_vm ON volumes(attached_vm_id);

CREATE TABLE IF NOT EXISTS volume_snapshots (
    id TEXT NOT NULL PRIMARY KEY,
    volume_id TEXT NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    atlas_snapshot_id TEXT,
    status TEXT NOT NULL DEFAULT 'available',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_volume_snapshots_volume ON volume_snapshots(volume_id);
