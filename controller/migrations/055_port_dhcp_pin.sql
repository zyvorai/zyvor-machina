-- Whether the port's reserved address is pinned as a DHCP host entry (MAC -> IP) on the subnet's libvirt network.
ALTER TABLE ports ADD COLUMN dhcp_pinned INTEGER NOT NULL DEFAULT 0;
