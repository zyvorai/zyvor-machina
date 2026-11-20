-- CloudWatch-style alarms on stored metric samples, with an optional step-scaling action on an instance group.
CREATE TABLE IF NOT EXISTS cloud_alarms (
    id BLOB NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    -- a VM name, or `group:<group id hex>` for every member of an instance group
    subject TEXT NOT NULL,
    metric TEXT NOT NULL,
    statistic TEXT NOT NULL DEFAULT 'Average',
    period_secs INTEGER NOT NULL DEFAULT 300,
    evaluation_periods INTEGER NOT NULL DEFAULT 1,
    comparator TEXT NOT NULL DEFAULT 'gt',
    threshold REAL NOT NULL,
    state TEXT NOT NULL DEFAULT 'INSUFFICIENT_DATA',
    state_reason TEXT NOT NULL DEFAULT '',
    state_updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- action: 'none' or 'scale_group' (changes the group's desired size by `step`, within its min/max)
    action TEXT NOT NULL DEFAULT 'none',
    group_id BLOB,
    step INTEGER NOT NULL DEFAULT 0,
    cooldown_secs INTEGER NOT NULL DEFAULT 300,
    last_action_at TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_cloud_alarms_group ON cloud_alarms(group_id);
