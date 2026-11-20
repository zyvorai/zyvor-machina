-- Platform ops: snapshots, backups, host notes, indexes

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS notes TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS snapshot_records (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS backup_records (
    id UUID PRIMARY KEY,
    vm_id UUID NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    backup_type TEXT NOT NULL DEFAULT 'full',
    status TEXT NOT NULL DEFAULT 'pending',
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_snapshot_vm ON snapshot_records(vm_id);
CREATE INDEX IF NOT EXISTS idx_backup_vm ON backup_records(vm_id);
CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_logs(created_at DESC);
