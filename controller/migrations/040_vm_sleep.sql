-- Scale-to-zero: idle VMs are managed-saved (desired_state='sleeping') and restored on traffic.
-- sleep_after_minutes: NULL inherits the project default, 0 never sleeps.
ALTER TABLE vms ADD COLUMN sleep_after_minutes INTEGER;
ALTER TABLE vms ADD COLUMN last_active_at TEXT;
ALTER TABLE vms ADD COLUMN slept_at TEXT;
ALTER TABLE vm_metrics ADD COLUMN net_bytes INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS vm_sleep_project_policies (
    project TEXT PRIMARY KEY,
    sleep_after_minutes INTEGER NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS vm_sleep_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_vm_sleep_events_vm ON vm_sleep_events(vm_id, at);
