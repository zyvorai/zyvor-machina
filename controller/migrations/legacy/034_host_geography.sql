-- Host geography for Mission Control / Infrastructure Earth (site → rack → U)

ALTER TABLE hosts ADD COLUMN IF NOT EXISTS site TEXT NOT NULL DEFAULT '';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS rack TEXT NOT NULL DEFAULT '';
ALTER TABLE hosts ADD COLUMN IF NOT EXISTS rack_u INT;

CREATE INDEX IF NOT EXISTS idx_hosts_site_rack ON hosts (site, rack);
