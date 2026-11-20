-- Phase 10: bare metal server inventory (Redfish/IPMI foundation)
CREATE TABLE IF NOT EXISTS baremetal_servers (
  id UUID PRIMARY KEY,
  hostname TEXT NOT NULL,
  bmc_address TEXT NOT NULL DEFAULT '',
  bmc_type TEXT NOT NULL DEFAULT 'redfish',
  state TEXT NOT NULL DEFAULT 'discovered',
  cpu_cores INT NOT NULL DEFAULT 0,
  memory_mib BIGINT NOT NULL DEFAULT 0,
  tags JSONB NOT NULL DEFAULT '[]'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_baremetal_servers_hostname ON baremetal_servers(hostname);
