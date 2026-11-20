-- Performance indexes: FK columns and common filter columns missing indexes.
-- All use IF NOT EXISTS so the migration is idempotent on new databases.

-- vm_disks: queried by vm_id on every VM detail load
CREATE INDEX IF NOT EXISTS idx_vm_disks_vm ON vm_disks(vm_id);

-- migration_jobs: vm timeline and history queries
CREATE INDEX IF NOT EXISTS idx_migration_jobs_vm ON migration_jobs(vm_id);
CREATE INDEX IF NOT EXISTS idx_migration_jobs_status ON migration_jobs(status);

-- network_reservations: IP lookup by VM and by network
CREATE INDEX IF NOT EXISTS idx_net_reservations_vm ON network_reservations(vm_id);
CREATE INDEX IF NOT EXISTS idx_net_reservations_network ON network_reservations(network_id);

-- application_group_vms: reverse lookup (which groups does a VM belong to?)
-- PK is (group_id, vm_id) so group→vm is covered; vm→group is not
CREATE INDEX IF NOT EXISTS idx_app_group_vms_vm ON application_group_vms(vm_id);

-- tasks: filter by resource (VM/host id) and by host
CREATE INDEX IF NOT EXISTS idx_tasks_resource ON tasks(resource_id, resource_type);
CREATE INDEX IF NOT EXISTS idx_tasks_host ON tasks(host_id);

-- snapshot_records: filter by status (pending/running)
CREATE INDEX IF NOT EXISTS idx_snapshot_status ON snapshot_records(status);

-- backup_records: filter by status
CREATE INDEX IF NOT EXISTS idx_backup_status ON backup_records(status);

-- soc_playbook_runs: filter by playbook
CREATE INDEX IF NOT EXISTS idx_soc_playbook_runs_playbook ON soc_playbook_runs(playbook_id, started_at DESC);

-- console_sessions: active sessions lookup
CREATE INDEX IF NOT EXISTS idx_console_sessions_active ON console_sessions(ended_at) WHERE ended_at IS NULL;

-- ai_conversations: filter by owner
CREATE INDEX IF NOT EXISTS idx_ai_conversations_owner ON ai_conversations(user_id, created_at DESC);

-- maintenance_windows: filter by host
CREATE INDEX IF NOT EXISTS idx_maintenance_windows_host ON maintenance_windows(host_id);
