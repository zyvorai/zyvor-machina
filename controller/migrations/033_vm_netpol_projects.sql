-- Fleet Cloud project networking: default isolation between projects,
-- per-project egress allowlists and egress IPs. Project `*` is the default
-- every project inherits isolation from.
CREATE TABLE IF NOT EXISTS vm_netpol_projects (
    project TEXT NOT NULL PRIMARY KEY,
    -- machina_bpf::netpol::tenant::ProjectNet as JSON
    settings TEXT NOT NULL,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
