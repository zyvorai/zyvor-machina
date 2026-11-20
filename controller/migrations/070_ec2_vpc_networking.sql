-- EC2 VPC networking objects. Internet gateways, route tables, ACLs and DHCP options are stored plans: nothing here
-- changes forwarding by itself (the NAT gateway drives the existing per-subnet host masquerade). Ids are UUIDs, shown as
-- igw-/nat-/rtb-/acl-/dopt- ids.
CREATE TABLE ec2_internet_gateways (
    id BLOB NOT NULL PRIMARY KEY,
    project_id BLOB,
    vpc_id BLOB UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE ec2_nat_gateways (
    id BLOB NOT NULL PRIMARY KEY,
    vpc_id BLOB NOT NULL,
    subnet_id BLOB NOT NULL,
    allocation_id TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'available',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_ec2_nat_gateways_vpc ON ec2_nat_gateways (vpc_id);
CREATE TABLE ec2_route_tables (
    id BLOB NOT NULL PRIMARY KEY,
    vpc_id BLOB NOT NULL,
    is_main BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_ec2_route_tables_vpc ON ec2_route_tables (vpc_id);
CREATE UNIQUE INDEX idx_ec2_route_tables_main ON ec2_route_tables (vpc_id) WHERE is_main = TRUE;
CREATE TABLE ec2_routes (
    id BLOB NOT NULL PRIMARY KEY,
    route_table_id BLOB NOT NULL,
    destination TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    target_id BLOB,
    UNIQUE (route_table_id, destination)
);
CREATE TABLE ec2_route_table_assocs (
    id BLOB NOT NULL PRIMARY KEY,
    route_table_id BLOB NOT NULL,
    subnet_id BLOB NOT NULL UNIQUE
);
CREATE INDEX idx_ec2_route_table_assocs_table ON ec2_route_table_assocs (route_table_id);
CREATE TABLE ec2_dhcp_options (
    id BLOB NOT NULL PRIMARY KEY,
    project_id BLOB,
    config TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE ec2_dhcp_assocs (
    vpc_id BLOB NOT NULL PRIMARY KEY,
    dhcp_options_id BLOB NOT NULL
);
CREATE TABLE ec2_network_acls (
    id BLOB NOT NULL PRIMARY KEY,
    vpc_id BLOB NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_ec2_network_acls_vpc ON ec2_network_acls (vpc_id);
CREATE UNIQUE INDEX idx_ec2_network_acls_default ON ec2_network_acls (vpc_id) WHERE is_default = TRUE;
CREATE TABLE ec2_network_acl_entries (
    acl_id BLOB NOT NULL,
    rule_number INTEGER NOT NULL,
    egress BOOLEAN NOT NULL,
    protocol TEXT NOT NULL,
    rule_action TEXT NOT NULL,
    cidr TEXT NOT NULL,
    port_from INTEGER,
    port_to INTEGER,
    PRIMARY KEY (acl_id, rule_number, egress)
);
CREATE TABLE ec2_acl_assocs (
    id BLOB NOT NULL PRIMARY KEY,
    acl_id BLOB NOT NULL,
    subnet_id BLOB NOT NULL UNIQUE
);
CREATE TABLE ec2_vpc_attrs (
    vpc_id BLOB NOT NULL PRIMARY KEY,
    dns_support BOOLEAN NOT NULL DEFAULT TRUE,
    dns_hostnames BOOLEAN NOT NULL DEFAULT FALSE
);
-- Secondary private addresses of a network interface: reserved in the subnet's address pool, not configured in the guest.
CREATE TABLE ec2_eni_ips (
    port_id BLOB NOT NULL,
    address TEXT NOT NULL,
    PRIMARY KEY (port_id, address)
);
