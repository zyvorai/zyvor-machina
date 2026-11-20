# Replacing OpenStack with Machina

Machina covers the private-cloud primitives a typical OpenStack deployment provides, with four Rust services instead of nine
or more plus Galera, RabbitMQ and Memcached. It is **not a full drop-in**: native clients should use Machina's REST API or
its [EC2-compatible API](../cloud-ec2-api.md). A [Keystone/Nova/Glance/Cinder/Neutron compatibility façade](../cloud-openstack-api.md)
also lets `openstack` CLI and Terraform's `openstack` provider drive the same Fleet Cloud resources. Guests can move as
disk images or through the first-party OpenStack importer.

![Replace OpenStack, service by service](../ux/readme-replace-openstack.jpg)

## Service map

| OpenStack | Machina | Notes |
|---|---|---|
| Keystone (identity) | PAM, OIDC, SAML, LDAP, RBAC, project-scoped API keys; Keystone v3 shim | See [LDAP](../ldap-auth.md), [scoped API keys](../guides/scoped-api-keys.md), [OpenStack API](../cloud-openstack-api.md). |
| Nova (compute) | `machina-daemon` + `machina-agent` on libvirt/KVM; the controller schedules; Nova v2.1 shim | Flavors are instance types. Live migration, HA failover and DRS are built in. |
| Neutron (networking) | Native eBPF VM edge and network policy, VPC objects, NAT, Elastic IPs; Neutron v2.0 shim | VPC peering, internet-gateway and blackhole routes and network ACLs are enforced on the hosts, across hosts over the WireGuard overlay: see [cloud-fabric.md](../cloud-fabric.md). No VXLAN/OVN tenant overlay. |
| Glance (images) | Images and templates, Golden Forge builds (virt-builder, Packer); Glance v2 shim | [Volumes and images](../guides/volumes-and-images.md) |
| Cinder (block storage) | Volumes with I/O limits and snapshots; Atlas for Ceph, NFS or ZFS; Cinder v3 shim | [Atlas](../atlas-storage.md) |
| Heat (orchestration) | Stacks with dry run, approval and drift repair | [Fleet Cloud features](../fleet-cloud-features.md#stacks-you-describe). Templates are not HOT. |
| Octavia (load balancing) | Native layer-4 balancer on the host, no amphora VM | `controller/src/engine/load_balancer.rs`; also reachable through the ELBv2 actions. |
| Horizon (dashboard) | The web UI, with browser consoles | No separate console proxy. |
| Masakari, Watcher | HA failover and DRS in the controller | [controller-ha.md](../controller-ha.md), [fleet-ha.md](../fleet-ha.md) |
| Ceilometer, Aodh | Metrics history, alarms with scaling actions, Prometheus and OTLP | [Alarms and scaling](../guides/alarms-and-scaling.md) |
| MariaDB/Galera, RabbitMQ | SQLite or PostgreSQL; in-memory task bus, NATS optional | [Database guide](../guides/database.md) |
| Magnum, Zun | KubeVirt, Podman and Docker (Vessel), `machina-cni` | Opt-in. |

## Concept map

| OpenStack | Machina / EC2 API |
|---|---|
| Project (tenant) | Fleet Cloud project; isolated from other projects by default |
| Flavor | Instance type |
| Security group | Security group, enforced on the host datapath |
| Floating IP | Elastic IP and NAT gateway (EC2 `AllocateAddress`) |
| Keypair | Key pair, injected at first boot |
| Server group | Server group |
| Heat stack | Stack |
| Cloud-init user data | User data, same format |

## Moving a workload

1. Inventory what you use. Anything on the "gaps" list below that you depend on decides whether Machina fits.
2. Stand up Machina next to OpenStack on spare hosts: `./machinactl deploy`, then add hosts ([Quickstart](../QUICKSTART.md)).
3. Re-create the shapes (projects, instance types, security groups, networks) through the web UI, the REST API, Terraform's
   `aws` provider against the [EC2 endpoint](../cloud-ec2-clients.md), or `openstack` / Terraform `openstack` against the
   [compatibility API](../cloud-openstack-api.md) (`OS_AUTH_URL=http://HOST:5093/v3`).
4. Move each guest: use **Platform → Migration → OpenStack** (first-party importer: inventory Glance/Nova/Cinder, download
   disks, register templates — API `/api/v1/openstack/…`, claim C75), or export a disk image (`openstack image save`) and
   land it with Import VM / `POST /api/v1/import/disk`. Transiva/h2kvm remains available for multi-hypervisor waves.
5. Cut traffic over (DNS or load balancer), then drain the old hosts and add them to Machina.

Live smoke helper: `scripts/openstack/mock-import-smoke.sh` (daemon cookie proxy or `MACHINA_CONTROLLER_URL`).

## Gaps

- Hyperscale multi-tenancy (thousands of hard-isolated tenants) and Neutron SDN breadth that needs OVN/VXLAN/BGP as in
  upstream Neutron remain out of scope; Machina expands networking on its eBPF + VPC fabric instead.
- The OpenStack compatibility API covers the smoke set in [cloud-openstack-api.md](../cloud-openstack-api.md), not every
  microversion or extension. Ecosystem projects (Trove, Sahara, Manila, …) have no counterpart.
- What has been run on a real host and what is unit-tested only is recorded in [claims.md](../claims.md).

Choose OpenStack when you run thousands of tenants, need OVN-grade SDN, or depend on its ecosystem. Machina targets fleets you
own: a lab, a branch, a sovereign region, a VMware exit.
