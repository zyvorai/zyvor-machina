-- vGPU (mediated device) partitions attached to VMs. A partition lives on one host's GPU and does
-- not move, so a VM holding one is pinned to its host (no migration, no DRS move, no HA restart
-- elsewhere). Partitions are not persistent across a host reboot; a row whose partition no longer
-- exists on the host is reported as missing.
CREATE TABLE vm_mdev (
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    uuid TEXT NOT NULL,
    mdev_type TEXT NOT NULL DEFAULT '',
    parent TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (vm_id, uuid)
);
-- A partition belongs to at most one VM on a host.
CREATE UNIQUE INDEX idx_vm_mdev_host_uuid ON vm_mdev(host_id, uuid);
