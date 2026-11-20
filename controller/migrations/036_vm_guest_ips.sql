-- Every guest address the agent saw (all NICs, IPv4 then IPv6) as a JSON array; guest_ip stays the primary IPv4.
ALTER TABLE vms ADD COLUMN guest_ips TEXT NOT NULL DEFAULT '[]';
