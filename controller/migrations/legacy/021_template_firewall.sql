-- Template firewall profiles for Zeus Firewall VM provisioning

ALTER TABLE templates ADD COLUMN IF NOT EXISTS firewall_profile TEXT;

UPDATE templates SET firewall_profile = 'WebServer'
WHERE name IN ('ubuntu-24.04', 'ubuntu-22.04', 'debian-12', 'fedora-40') AND firewall_profile IS NULL;

UPDATE templates SET firewall_profile = 'ProductionServer'
WHERE name IN ('centos-stream-9', 'rocky-9', 'alma-9') AND firewall_profile IS NULL;

UPDATE templates SET firewall_profile = 'DatabaseServer'
WHERE (name LIKE '%postgres%' OR name LIKE '%mysql%' OR category = 'Database') AND firewall_profile IS NULL;

UPDATE templates SET firewall_profile = 'ManagementNode'
WHERE name LIKE '%windows%' AND firewall_profile IS NULL;
