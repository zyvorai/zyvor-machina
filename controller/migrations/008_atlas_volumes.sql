-- Copyright (c) 2026 ZyvorAI Labs Private Limited. All rights reserved.
-- Atlas storage integration: bind machina VMs to Atlas-provisioned backend
-- volumes (Ceph RBD / NFS / ZFS) so snapshot / backup / restore can be routed
-- through the Atlas control plane. Atlas owns the volume; machina owns the
-- binding (which VM disk it is) and the RBD source used to build domain XML.

CREATE TABLE IF NOT EXISTS vm_atlas_volumes (
    id TEXT NOT NULL PRIMARY KEY CHECK(length(id) = 16),
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    -- Atlas volume id, e.g. "vol_b16c40e12b76".
    volume_id TEXT NOT NULL,
    -- role of the disk in the VM: root_disk | data_disk.
    role TEXT NOT NULL DEFAULT 'data_disk',
    size_bytes INTEGER NOT NULL DEFAULT 0,
    policy TEXT NOT NULL DEFAULT 'general',
    -- backend-native reference used to build the libvirt disk source, e.g.
    -- "rbd-nvme-prod/csi-vol-…" for an RBD image.
    backend_native_id TEXT,
    state TEXT NOT NULL DEFAULT 'provisioning',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_vm_atlas_volumes_vm ON vm_atlas_volumes(vm_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_vm_atlas_volumes_volume ON vm_atlas_volumes(volume_id);
