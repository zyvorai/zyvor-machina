# Network interfaces, Elastic IPs, NAT gateway, subnet delete

## What it is
Four pieces of EC2 networking for cloud subnets: network interfaces (ports) with a reserved, DHCP-pinned address; Elastic IPs
(public addresses mapped 1:1 to an instance); a NAT gateway so a private subnet can reach the outside; and subnet delete with
libvirt teardown.

## Configure
- A VPC and a ready subnet (`/api/v1/cloud/vpcs`, `/cloud/subnets`).
- **Elastic IPs** need a pool an admin defines: `POST /api/v1/elastic-ip-pools {name, cidr, host_id, interface}`. The address must
  be routed to that host by your network; the interface name is at most 11 characters.
- **NAT** is per subnet and needs the subnet's instances to have a default route via the subnet's `.1` address (set it in
  cloud-init network config or user-data; machina does not install it).

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
mc -X POST $API/ports -d '{"subnet_id":"<subnet>","private_ip":"10.250.1.20"}'   # reserves the address, pins it in DHCP
mc -X POST $API/elastic-ips -d '{}'                                             # allocate from the first pool
mc -X POST $API/elastic-ips/<id or address>/associate -d '{"vm_id":"<vm>"}'
mc -X POST $API/elastic-ips/<id>/disassociate
mc -X DELETE $API/elastic-ips/<id>                                                # release (must be unassociated)
mc -X PUT $API/cloud/subnets/<id>/nat -d '{"enabled":true}'
mc -X DELETE $API/cloud/subnets/<id>                                              # refused while it is in use
```
Responses say whether the host confirmed the change (`applied`, `apply_error`).

## Check it works
On the host: `sudo iptables -t nat -S MACHINA_EIP_DNAT` shows the mapping, `ip addr show <iface>` the `:eip` alias, and
`MACHINA_NAT` the masquerade rule. The agent rebuilds these chains every 30 s, so a manual flush heals itself. Unit tests:
`cargo test -p machina-controller pick_address_tests engine::eip elastic_ips subnet_delete` and
`cargo test -p machina-agent eip::tests natgw`.

## Limits
Host-local and IPv4 only: an Elastic IP follows no instance to another host. No secondary addresses; a port carries one security
group. Subnet delete is refused (409) while addresses, interfaces, groups or instances remain, and VPC delete while subnets or
peerings remain.
