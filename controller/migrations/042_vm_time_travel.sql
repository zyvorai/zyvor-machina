-- Time travel: restore points are the frozen qcow2 layer files of each disk;
-- forks record which point their disks sit on (pinning it).
-- restore_point_minutes: NULL or 0 = no scheduled restore points.
ALTER TABLE vms ADD COLUMN restore_point_minutes INTEGER;
ALTER TABLE vms ADD COLUMN restore_point_keep INTEGER;

CREATE TABLE IF NOT EXISTS vm_restore_points (
    id TEXT PRIMARY KEY,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'manual',
    note TEXT,
    layers TEXT NOT NULL DEFAULT '[]',
    quiesced INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_vm_restore_points_vm ON vm_restore_points(vm_id, created_at);

CREATE TABLE IF NOT EXISTS vm_forks (
    fork_vm_id TEXT PRIMARY KEY REFERENCES vms(id) ON DELETE CASCADE,
    source_vm_id TEXT NOT NULL,
    restore_point_id TEXT,
    memory INTEGER NOT NULL DEFAULT 0,
    isolated INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_vm_forks_source ON vm_forks(source_vm_id);
