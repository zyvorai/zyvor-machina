-- Auto Scaling service (EC2 query API, service `autoscaling`) on top of the instance groups in cloud_instance_groups.
-- The group row owns capacity (policy_json) and members; these tables hold what the Auto Scaling API adds.
CREATE TABLE asg_groups (
    group_id TEXT PRIMARY KEY NOT NULL REFERENCES cloud_instance_groups(id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE,
    launch_config_name TEXT NOT NULL DEFAULT '',
    launch_template_version TEXT NOT NULL DEFAULT '$Default',
    health_check_type TEXT NOT NULL DEFAULT 'EC2',
    health_check_grace INTEGER NOT NULL DEFAULT 0,
    suspended TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE asg_launch_configs (
    name TEXT PRIMARY KEY NOT NULL,
    image_id TEXT NOT NULL,
    instance_type TEXT NOT NULL DEFAULT '',
    key_name TEXT NOT NULL DEFAULT '',
    user_data TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE asg_tags (
    group_id TEXT NOT NULL REFERENCES asg_groups(group_id) ON DELETE CASCADE,
    tag_key TEXT NOT NULL,
    tag_value TEXT NOT NULL DEFAULT '',
    propagate_at_launch INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (group_id, tag_key)
);
CREATE TABLE asg_policies (
    id TEXT PRIMARY KEY NOT NULL,
    group_id TEXT NOT NULL REFERENCES asg_groups(group_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    policy_type TEXT NOT NULL,
    adjustment_type TEXT NOT NULL DEFAULT '',
    scaling_adjustment INTEGER NOT NULL DEFAULT 0,
    min_adjustment_magnitude INTEGER NOT NULL DEFAULT 0,
    cooldown INTEGER NOT NULL DEFAULT 0,
    steps_json TEXT NOT NULL DEFAULT '[]',
    target_value REAL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (group_id, name)
);
CREATE TABLE asg_activities (
    id TEXT PRIMARY KEY NOT NULL,
    group_id TEXT NOT NULL REFERENCES asg_groups(group_id) ON DELETE CASCADE,
    description TEXT NOT NULL,
    cause TEXT NOT NULL,
    status_code TEXT NOT NULL DEFAULT 'Successful',
    status_message TEXT NOT NULL DEFAULT '',
    start_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_asg_activities_group ON asg_activities (group_id, start_time);
