-- VM inventory reconcile: libvirt tombstones + KubeVirt platform inventory

ALTER TABLE clusters ADD COLUMN IF NOT EXISTS inventory_prune_unmanaged BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE clusters ADD COLUMN IF NOT EXISTS inventory_mark_managed_missing BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE vms ADD COLUMN IF NOT EXISTS inventory_source TEXT NOT NULL DEFAULT 'libvirt';
ALTER TABLE vms ADD COLUMN IF NOT EXISTS last_seen_at TIMESTAMPTZ;
ALTER TABLE vms ADD COLUMN IF NOT EXISTS k8s_namespace TEXT;
ALTER TABLE vms ADD COLUMN IF NOT EXISTS k8s_uid TEXT;

-- Drop global name uniqueness so libvirt and kubevirt names can coexist.
ALTER TABLE vms DROP CONSTRAINT IF EXISTS vms_cluster_id_name_key;

CREATE UNIQUE INDEX IF NOT EXISTS idx_vms_libvirt_name
    ON vms (cluster_id, name)
    WHERE inventory_source = 'libvirt';

CREATE UNIQUE INDEX IF NOT EXISTS idx_vms_kubevirt_name
    ON vms (cluster_id, k8s_namespace, name)
    WHERE inventory_source = 'kubevirt';

CREATE INDEX IF NOT EXISTS idx_vms_inventory_source ON vms (inventory_source);
CREATE INDEX IF NOT EXISTS idx_vms_observed_state ON vms (observed_state);
CREATE INDEX IF NOT EXISTS idx_vms_host_source ON vms (host_id, inventory_source);
