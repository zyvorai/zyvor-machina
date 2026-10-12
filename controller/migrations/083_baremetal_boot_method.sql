-- How a server reaches the boot service: `pxe` (DHCP/TFTP on its network) or
-- `virtual_media` (the BMC mounts a small iPXE ISO, so no DHCP/TFTP is needed).
ALTER TABLE baremetal_servers ADD COLUMN boot_method TEXT NOT NULL DEFAULT 'pxe';
