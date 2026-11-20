-- EC2 objects and attributes that have no column of their own: per-instance attributes, launch template versions,
-- placement groups, spot requests, fleets, volume type and modification history.
CREATE TABLE ec2_instance_attrs (
    vm_id TEXT NOT NULL PRIMARY KEY REFERENCES vms(id) ON DELETE CASCADE,
    disable_api_termination BOOLEAN NOT NULL DEFAULT FALSE,
    ebs_optimized BOOLEAN NOT NULL DEFAULT FALSE,
    monitoring BOOLEAN NOT NULL DEFAULT FALSE,
    metadata_endpoint BOOLEAN NOT NULL DEFAULT TRUE,
    metadata_hop_limit BIGINT NOT NULL DEFAULT 1,
    placement_group TEXT
);
CREATE INDEX idx_ec2_instance_attrs_pg ON ec2_instance_attrs (placement_group);

ALTER TABLE cloud_launch_templates ADD COLUMN default_version BIGINT NOT NULL DEFAULT 1;
CREATE TABLE ec2_launch_template_versions (
    template_id TEXT NOT NULL REFERENCES cloud_launch_templates(id) ON DELETE CASCADE,
    version BIGINT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    data_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (template_id, version)
);

CREATE TABLE ec2_placement_groups (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    strategy TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE ec2_fleets (
    id TEXT NOT NULL PRIMARY KEY,
    fleet_type TEXT NOT NULL,
    state TEXT NOT NULL,
    target_capacity BIGINT NOT NULL,
    spot BOOLEAN NOT NULL DEFAULT FALSE,
    template_id TEXT,
    template_version TEXT NOT NULL DEFAULT '$Default',
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE ec2_fleet_instances (
    fleet_id TEXT NOT NULL REFERENCES ec2_fleets(id) ON DELETE CASCADE,
    vm_id TEXT NOT NULL,
    PRIMARY KEY (fleet_id, vm_id)
);

CREATE TABLE ec2_spot_requests (
    id TEXT NOT NULL PRIMARY KEY,
    request_type TEXT NOT NULL DEFAULT 'one-time',
    state TEXT NOT NULL,
    status_code TEXT NOT NULL DEFAULT '',
    status_message TEXT NOT NULL DEFAULT '',
    vm_id TEXT,
    fleet_id TEXT,
    spot_price TEXT NOT NULL DEFAULT '',
    instance_type TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_ec2_spot_requests_vm ON ec2_spot_requests (vm_id);

CREATE TABLE ec2_volume_attrs (
    volume_id TEXT NOT NULL PRIMARY KEY REFERENCES volumes(id) ON DELETE CASCADE,
    volume_type TEXT NOT NULL DEFAULT 'gp2',
    iops BIGINT,
    throughput BIGINT
);
CREATE TABLE ec2_volume_modifications (
    id TEXT NOT NULL PRIMARY KEY,
    volume_id TEXT NOT NULL,
    original_size BIGINT NOT NULL,
    target_size BIGINT NOT NULL,
    original_type TEXT NOT NULL,
    target_type TEXT NOT NULL,
    original_iops BIGINT,
    target_iops BIGINT,
    original_throughput BIGINT,
    target_throughput BIGINT,
    state TEXT NOT NULL DEFAULT 'completed',
    start_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    end_time TEXT
);
CREATE INDEX idx_ec2_volume_modifications_volume ON ec2_volume_modifications (volume_id);
