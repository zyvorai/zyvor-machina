-- Phase 23: bare metal Zeus Firewall policy columns (AI-312–331)
ALTER TABLE baremetal_servers ADD COLUMN IF NOT EXISTS firewall_profile TEXT NOT NULL DEFAULT 'BareMetalBmc';
ALTER TABLE baremetal_servers ADD COLUMN IF NOT EXISTS firewall_enabled BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE baremetal_servers ADD COLUMN IF NOT EXISTS bmc_vlan TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN IF NOT EXISTS pxe_vlan TEXT NOT NULL DEFAULT '';
ALTER TABLE baremetal_servers ADD COLUMN IF NOT EXISTS posture_json JSONB;
ALTER TABLE baremetal_servers ADD COLUMN IF NOT EXISTS last_exposure_scan_at TIMESTAMPTZ;

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS baremetal_origin_id UUID REFERENCES baremetal_servers(id);
