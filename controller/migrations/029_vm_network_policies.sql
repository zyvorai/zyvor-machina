-- VM network policies (CiliumNetworkPolicy schema, VMs as endpoints) and
-- key/value VM labels that their selectors match.

ALTER TABLE vms ADD COLUMN labels TEXT NOT NULL DEFAULT '{}';

-- Existing `key=value` tags become labels.
UPDATE vms
SET labels = COALESCE(
    (SELECT json_group_object(substr(value, 1, instr(value, '=') - 1), substr(value, instr(value, '=') + 1))
     FROM json_each(vms.tags)
     WHERE instr(value, '=') > 1),
    '{}')
WHERE tags IS NOT NULL AND tags != '[]';

CREATE TABLE IF NOT EXISTS vm_network_policies (
    name TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'VmNetworkPolicy',
    -- machina_bpf::netpol::VmNetworkPolicy as JSON
    policy_json TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_by TEXT NOT NULL DEFAULT '',
    generation INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS vm_netpol_host_status (
    host_id TEXT NOT NULL PRIMARY KEY,
    hostname TEXT NOT NULL DEFAULT '',
    synced_at TEXT,
    ok INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    vms INTEGER NOT NULL DEFAULT 0,
    rules INTEGER NOT NULL DEFAULT 0,
    peers INTEGER NOT NULL DEFAULT 0,
    generation INTEGER NOT NULL DEFAULT 0,
    warnings TEXT NOT NULL DEFAULT '[]'
);
