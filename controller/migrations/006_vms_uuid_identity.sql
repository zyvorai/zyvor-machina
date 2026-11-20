-- A libvirt VM's stable identity is its UUID, not (cluster_id, name). The old
-- (cluster_id, name) unique index collapsed two DIFFERENT VMs that happened to
-- share a name on different hosts into one row (host_id flip-flopping every
-- inventory tick, one VM permanently invisible). Key uniqueness on the UUID
-- instead: a live-migrated VM keeps its row (same uuid, new host_id), while two
-- same-name VMs on different hosts stay distinct rows.
--
-- The partial WHERE excludes empty/NULL uuids so rows without a reported uuid
-- (rare for real libvirt domains) are unconstrained and fall back to name
-- matching, and so this CREATE can't fail on pre-existing empty-uuid rows.
DROP INDEX IF EXISTS idx_vms_libvirt_name;

CREATE UNIQUE INDEX IF NOT EXISTS idx_vms_libvirt_uuid
    ON vms(cluster_id, uuid)
    WHERE inventory_source = 'libvirt' AND uuid IS NOT NULL AND uuid != '';
