-- NAT gateway: a private cloud subnet whose instances reach the outside masqueraded behind the host's uplink.
ALTER TABLE cloud_subnets ADD COLUMN nat_enabled INTEGER NOT NULL DEFAULT 0;
