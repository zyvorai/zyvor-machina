-- ENI semantics on ports: a port on a cloud subnet reserves a managed address (cloud_ip_allocations,
-- request_key 'eni-<port id>') that is released when the port is deleted.
ALTER TABLE ports ADD COLUMN subnet_id TEXT;
ALTER TABLE ports ADD COLUMN private_ip TEXT;
ALTER TABLE ports ADD COLUMN description TEXT NOT NULL DEFAULT '';
