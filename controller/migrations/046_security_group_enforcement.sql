-- EC2-style security groups that the VM edge can enforce.
-- `mode`: 'audit' = stored and advisory (every pre-existing group), 'enforce' = compiled into a managed
-- network policy per attached VM. Ids are stored like everywhere else (sqlx Uuid = 16-byte BLOB);
-- rules reference a peer group by `remote_sg_id` (simple hex). Nothing changes for existing groups until an operator flips the mode.
ALTER TABLE security_groups ADD COLUMN mode TEXT NOT NULL DEFAULT 'audit';
ALTER TABLE security_group_rules ADD COLUMN remote_sg_id TEXT;
ALTER TABLE security_group_rules ADD COLUMN description TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS instance_security_groups (
    vm_id BLOB NOT NULL,
    sg_id BLOB NOT NULL REFERENCES security_groups(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (vm_id, sg_id)
);
CREATE INDEX IF NOT EXISTS idx_isg_sg ON instance_security_groups(sg_id);
