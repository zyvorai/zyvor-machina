-- WireGuard overlay between hosts: settings (key `settings`, JSON) and
-- stable allocations: `host:<host id>` = the host's prefix index,
-- `a4:<host id>:<address>` / `a6:<host id>:<address>` = a VM address's slot.
CREATE TABLE IF NOT EXISTS vm_netpol_overlay (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
