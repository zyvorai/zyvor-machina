---
sidebar_position: 4
title: Replacing OpenStack
description: Service map, OpenStack-compatible API, first-party importer, and what Machina still does not cover.
---

# Replacing OpenStack

![Replace OpenStack, service by service](/readme-replace-openstack.jpg)

Machina covers the private-cloud primitives a typical OpenStack deployment provides, with **four Rust services** instead of
nine or more plus Galera, RabbitMQ and Memcached. It is **not a full drop-in** of every OpenStack extension: prefer Machina's
REST API or its [EC2-compatible API](../core-concepts/ec2-api.md) for new automation. A
[Keystone / Nova / Glance / Cinder / Neutron façade](../core-concepts/openstack-api.md) on the controller also lets
`openstack` CLI and Terraform's `openstack` provider drive the same Fleet Cloud resources. Guests move as disk images or
through the first-party OpenStack importer.

![Nine OpenStack services collapse into four](/anim/openstack-vs-machina.svg)

## Service map

| OpenStack | Machina |
|---|---|
| Keystone | PAM, OIDC, SAML, LDAP, RBAC, project-scoped API keys; **Keystone v3 shim** |
| Nova, Placement | `machina-daemon` + `machina-agent` on libvirt/KVM; controller schedules (live migration, HA, DRS); **Nova v2.1 shim** |
| Neutron | Native eBPF VM edge and network policy, VPC objects, NAT, Elastic IPs, WireGuard overlay; **Neutron v2.0 shim** |
| Glance | Images and templates, Golden Forge builds; **Glance v2 shim** |
| Cinder | Volumes with I/O limits and snapshots; Atlas for Ceph, NFS or ZFS; **Cinder v3 shim** |
| Heat | Stacks with dry run, approval and drift repair (not HOT templates) |
| Octavia | Native layer-4 balancer on the host, no amphora VM |
| Horizon | The web UI, with browser consoles |
| Masakari, Watcher | HA failover and DRS in the controller |
| Ceilometer, Aodh | Metrics history, alarms with scaling actions, Prometheus and OTLP |
| MariaDB, RabbitMQ | [SQLite or PostgreSQL](database.md); in-memory task bus, NATS optional |

## Concept map

| OpenStack | Machina / EC2 API |
|---|---|
| Project (tenant) | Fleet Cloud project (isolated by default) |
| Flavor | Instance type |
| Security group | Security group, enforced on the host datapath |
| Floating IP | Elastic IP and NAT gateway |
| Keypair | Key pair, injected at first boot |
| Server group | Server group (anti-affinity via the Nova shim) |
| Heat stack | Stack |
| Cloud-init user data | User data, same format |

## Moving a workload

1. Inventory what you use. Anything on the gaps list below that you depend on decides whether Machina fits.
2. Stand up Machina next to OpenStack on spare hosts ([Quickstart](quickstart.md)).
3. Re-create projects, instance types, security groups and networks through the web UI, the REST API, Terraform's `aws`
   provider against the [EC2 endpoint](../core-concepts/ec2-api.md), or `openstack` / Terraform `openstack` against the
   [compatibility API](../core-concepts/openstack-api.md).
4. Move each guest:
   - **Platform → Migration → OpenStack** — first-party importer: connect to Keystone, sync Glance/Nova/Cinder inventory,
     download Glance images, register Fleet Cloud templates; or
   - export a disk (`openstack image save`) and land it with Import VM / `POST /api/v1/import/disk`; or
   - use Transiva/h2kvm for multi-hypervisor waves ([Migrating VMs in](migrate-vms-in.md)).
5. Cut traffic over (DNS or load balancer), then drain the old hosts and add them to Machina.

## OpenStack-compatible API (summary)

Point `OS_AUTH_URL` at `http://HOST:5093/v3` (tunnel `:15093` in labs). Password auth uses controller local users;
application credentials accept Machina API keys. Catalog advertises Nova, Glance, Cinder and Neutron on the same host.
Details, smoke commands and limits: [OpenStack-compatible API](../core-concepts/openstack-api.md).

## Gaps (precise)

- Hyperscale multi-tenancy (thousands of hard-isolated tenants) and Neutron SDN that needs **OVN / VXLAN / BGP-as-Neutron**
  remain out of scope; Machina expands networking on its eBPF + VPC fabric ([VPC forwarding](../core-concepts/vpc-forwarding.md)).
- The compatibility API covers the documented smoke set, not every microversion or extension. Ecosystem projects (Trove,
  Sahara, Manila, Magnum-as-OpenStack, amphora Octavia, Heat HOT) have no counterpart.
- Import from a **production** OpenStack cloud has not been run end to end yet; mock import and live API smoke are recorded
  in the [claims ledger](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md) (C74–C76).

Choose OpenStack when you run thousands of tenants, need OVN-grade SDN, or depend on its ecosystem. Machina targets fleets
you own: a lab, a branch, a sovereign region, a VMware exit.

Full engineering map (same content, repository paths):
[docs/migration/from-openstack.md](https://github.com/zyvorai/zyvor-machina/blob/main/docs/migration/from-openstack.md).
