-- VM power/snapshot schedules (per-VM recurring actions)
CREATE TABLE IF NOT EXISTS vm_schedules (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    action TEXT NOT NULL CHECK(action IN ('start', 'shutdown', 'stop', 'snapshot')),
    interval_minutes INTEGER NOT NULL DEFAULT 1440,
    retention INTEGER,
    label TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    next_run_at TEXT NOT NULL,
    last_run_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_vm_schedules_next_run
    ON vm_schedules(next_run_at) WHERE enabled = 1;

CREATE INDEX IF NOT EXISTS idx_vm_schedules_vm
    ON vm_schedules(vm_id);
