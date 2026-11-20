-- Day-2: autonomous health watchdog (opt-in per VM). HA handles HOST failure (fence +
-- recover); reconcile handles desired!=observed power drift. Neither handles a VM that is
-- still "running" at the hypervisor but whose GUEST has hung (guest agent went unreachable).
-- The watchdog hard-resets such a VM, bounded by a cooldown and a per-hour cap, and only for
-- VMs the operator explicitly opted in (default disabled — auto-restart is never implicit).
CREATE TABLE IF NOT EXISTS vm_watchdog (
    vm_id TEXT NOT NULL PRIMARY KEY REFERENCES vms(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 0,
    -- how long the guest must be continuously unhealthy before we act
    failure_threshold_secs INTEGER NOT NULL DEFAULT 120,
    -- minimum gap between two auto-resets of the same VM
    cooldown_secs INTEGER NOT NULL DEFAULT 600,
    -- safety cap so a crash-looping VM isn't reset forever
    max_restarts_per_hour INTEGER NOT NULL DEFAULT 3,
    unhealthy_since TEXT,
    last_restart_at TEXT,
    restarts_this_hour INTEGER NOT NULL DEFAULT 0,
    hour_window_start TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_vm_watchdog_enabled ON vm_watchdog(enabled);
