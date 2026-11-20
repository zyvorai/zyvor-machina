-- FluxVM VMs reported by each host agent (inventory_source = 'fluxvm'). The last record seen is kept so HA can
-- re-create a VM on another host after its host is gone; engine and storage say whether it can migrate.
ALTER TABLE vms ADD COLUMN fluxvm_engine TEXT;
ALTER TABLE vms ADD COLUMN fluxvm_storage TEXT;
ALTER TABLE vms ADD COLUMN fluxvm_record_json TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS idx_vms_fluxvm_name ON vms(cluster_id, name) WHERE inventory_source = 'fluxvm';
