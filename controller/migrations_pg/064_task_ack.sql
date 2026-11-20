-- An operator can acknowledge failed tasks so the failure counter shows what is new, not history.
ALTER TABLE tasks ADD COLUMN acknowledged_at TEXT;
CREATE INDEX idx_tasks_failed_open ON tasks (status, acknowledged_at);
