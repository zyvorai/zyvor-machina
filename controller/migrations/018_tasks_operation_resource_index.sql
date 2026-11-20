-- reconcile_once (controller/src/engine/reconcile.rs) runs, every 60s tick,
-- `SELECT resource_id, MAX(created_at) FROM tasks WHERE operation = 'vm.power'
--  GROUP BY resource_id` joined against every out-of-sync VM. The existing
-- idx_tasks_operation(operation) index alone still requires scanning every
-- 'vm.power' row to group/aggregate; this compound index lets SQLite satisfy
-- the WHERE + GROUP BY + MAX() directly from the index without a full table
-- scan, keeping the per-tick cost roughly constant as task history grows.
CREATE INDEX IF NOT EXISTS idx_tasks_operation_resource_created
    ON tasks(operation, resource_id, created_at);
