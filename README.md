<div align="center">

# Machina

[![CI](https://github.com/zyvorai/zyvor-machina/actions/workflows/ci.yml/badge.svg)](https://github.com/zyvorai/zyvor-machina/actions/workflows/ci.yml)
[![License: Zyvor Production v1.0](https://img.shields.io/badge/License-Zyvor%20Production%20v1.0-orange.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-daemon%20%C2%B7%20controller%20%C2%B7%20agent%20%C2%B7%20bpfd-000000?logo=rust)](Cargo.toml)
[![KVM](https://img.shields.io/badge/libvirt-QEMU%2FKVM-2997ff)](docs/README.md)
[![Website](https://img.shields.io/badge/Website-zyvorai.github.io%2Fzyvor--machina-0071e3)](https://zyvorai.github.io/zyvor-machina/)

[![Book a demo](https://img.shields.io/badge/Book_a_demo-0071e3?style=for-the-badge)](https://zyvor.dev/schedule?utm_source=github&utm_medium=machina&utm_campaign=readme_hero)
[![30-day PoC](https://img.shields.io/badge/30--day_PoC-000000?style=for-the-badge)](https://zyvor.dev/poc?utm_source=github&utm_medium=machina&utm_campaign=readme_hero)
[![Quickstart](https://img.shields.io/badge/Quickstart_in_one_command-2997ff?style=for-the-badge)](#quickstart)

![Machina: run your own cloud on your own hardware](docs/ux/readme-hero.jpg)

![Deploy Machina on one host with one command](docs/social/anim/deploy-single-host.svg)

### Your metal. Your cloud. One control plane.

**The private cloud you can install before lunch.** VMs, browser consoles, fleet HA/DRS, an OpenStack-style self-service cloud, a native eBPF datapath and AI operations, from a handful of Rust services on plain Linux + KVM.

**One-command install** · **SQLite built in, PostgreSQL when you grow** · **EC2-compatible API** · **A way off OpenStack** · **Consoles built in** · **eBPF networking built in** · **AI that asks before it acts**

</div>

---

## What's new

**Bare-metal provisioning.** Discover or import BMCs, commission machines over PXE or BMC virtual media, write a
checksum-verified OS image and let each machine join the fleet on first boot, driven from `machinactl metal …` or the
Bare Metal page. Verified on VMs and BMC emulators, **not yet on physical servers**; see the
[guide](docs/baremetal-provisioning.md) and [claims C49 to C51](docs/claims.md).

**EC2-style Fleet Cloud.** Launch several instances at once with a key pair and first-boot user data, tag everything,
firewall with **enforced security groups whose status tells the truth**, attach volumes with live I/O limits, scale groups from
alarms, give a team a project-scoped API key, and drive it all from `awscli` or boto3 against the built-in EC2-compatible endpoint.
Start with the [tutorials](docs/tutorials/README.md); every feature has a [guide](docs/guides/README.md), and
[what is proven on a real host and what is not](docs/claims.md) is written down.

VPC foundations and elastic compute are available as a first host-local backend.
See [the operator guide](docs/cloud-vpc-elastic-compute.md) for isolation, deployment,
CPU scaling, and the explicit routing/peering limitations.

| | |
|---|---|
| **Scale to zero** | Idle VMs sleep (memory saved to disk, RAM handed back to the host) and wake on the first packet sent to them. [How →](docs/fleet-cloud-features.md#scale-to-zero) |
| **Time travel** | Fork a running VM in seconds, with or without its RAM; scheduled restore points and one-click rewind. [How →](docs/fleet-cloud-features.md#time-travel) |
| **Stacks you describe** | Say what you want in a sentence; get a template, a dry run (quota, placement, cost, policy replay), an approval, then a stack that heals its own drift. [How →](docs/fleet-cloud-features.md#stacks-you-describe) |
| **Autopilot capacity** | Instance groups scale ahead of the daily or weekly rush, drain from their load balancer, and sleep instead of stopping; VMs are right-sized from their own history and hosts consolidated, each through an approval you can undo. [How →](docs/fleet-cloud-features.md#autopilot-capacity) |
| **Game days** | Inject latency, loss, partitions, slow disks and crashes into your own VMs under a lease; health probes abort the run and lift every fault when the service suffers; a report per step. [How →](docs/fleet-cloud-features.md#game-days) |
| **Preemptible instances** | VMs that can wait give way when a host runs short of memory: saved to disk instead of stopped, back by themselves, lowest priority first, when there is room. [How →](docs/fleet-cloud-features.md#preemptible-instances) |
| **Boot Doctor** | A VM that won't boot gets diagnosed and repaired offline through GuestKit, with a backup taken first. |
| **VM quarantine** | Isolate a suspect VM in the eBPF datapath under a time-boxed lease that lapses on its own. |
| **Just-in-time access** | Open a VM's network for a set time, with two-person approval and automatic expiry. |
| **Zyra verify and undo** | Every AI action is checked afterwards ("did it work?") and can be rolled back in one click. |
| **VM flow history** | Service map, learn and replay of VM traffic, with lateral-movement alerts. |
| **DNS threat feeds** | VM network policy blocks known-bad domains from threat feeds. |
| **Plain-English policies** | Describe a VM network policy in words; Zyvor drafts the YAML, dry-runs it against past traffic and applies it after approval. |
| **Project isolation** | Fleet Cloud projects are isolated from each other by default, with per-project egress allowlists and egress IPs that Machina adds to the host and announces itself. |
| **Encrypted cross-host overlay** | WireGuard between hypervisors; VM traffic keeps its identity across hosts, so isolation holds even on NAT networks. |
| **Segmentation evidence** | Export a sealed (SHA-256) JSON or Markdown report of policies, the reachability matrix and denied traffic for audits. |
| **FluxVM backend** | VMs from the FluxVM engine appear next to libvirt VMs, with eBPF networking, live migration and HA across hosts. [Guide →](docs/fluxvm.md) |
| **PostgreSQL controller** | Run the controller on PostgreSQL for large fleets or several controllers; `machinactl db setup pod --migrate` moves an SQLite site over. [Database →](#database-sqlite-or-postgresql) |
| **Quick create and Fix-it** | Image, size, Create; then a one-click Fix-it list (guest agent, backups) in Command Center. |

---

## Why Machina

| When this happens… | Machina gives you… |
|---|---|
| You want a private cloud, but OpenStack is a six-week project and a full-time team | `./machinactl deploy`: a few Rust services, embedded SQLite (PostgreSQL when you outgrow it), a browser UI on `:5092` minutes later |
| VMware renewal quotes keep climbing | Open KVM/libvirt underneath, with HA failover, DRS and live migration on top |
| libvirt ops live in a pile of `virsh` scripts | One dashboard, a REST API with 1,000+ routes, a CLI and a Terraform provider over the same model |
| Every console needs its own gateway | noVNC, SPICE, serial and SSH proxied by the daemon, with RBAC and audit |
| Networking means Cilium + Tetragon + kube-proxy + a firewall agent | One eBPF service, `machina-bpfd`: load balancing, DDoS shield, VM isolation, flow visibility |
| On-call means triaging the same incidents at 3 a.m. | Zyra AI diagnoses, correlates and proposes the fix, then waits for a human approval |

**Evaluating?** [Why Machina](docs/buyers/why-machina.md) (five minutes) · [Alternatives](docs/buyers/machina-vs-alternatives.md) · [Security and compliance](docs/buyers/security-and-compliance.md) · [30-day evaluation plan](docs/buyers/evaluation-guide.md). **Using it?** [Tutorials](docs/tutorials/README.md) · [Cookbook](docs/users/cookbook.md).

![Capabilities at a glance: Run, Secure, Scale, Operate](docs/ux/readme-capabilities.jpg)

---

## Replace OpenStack

![Replace OpenStack, service by service](docs/ux/readme-replace-openstack.jpg)

![Nine OpenStack services collapse into four Machina services](docs/social/anim/openstack-vs-machina.svg)

Machina covers the private-cloud primitives most OpenStack deployments use: identity, compute, networking, images, volumes,
orchestration, load balancing, a dashboard, HA and alarms. It is **not a drop-in**: there is no Nova or Neutron API, so tools
move to Machina's REST API or its [EC2-compatible API](#ec2-compatible-api), and guests move as disk images.

| OpenStack | Machina |
|---|---|
| Keystone | PAM, OIDC, SAML, LDAP, RBAC, project-scoped API keys |
| Nova, Placement | `machina-daemon` and `machina-agent` on libvirt/KVM; the controller schedules, with live migration, HA and DRS |
| Neutron | Native eBPF VM edge and network policy, VPC objects, NAT, Elastic IPs, WireGuard overlay between hosts |
| Glance | Images and templates, Golden Forge builds |
| Cinder | Volumes with I/O limits and snapshots; Atlas for Ceph, NFS or ZFS |
| Heat | Stacks with dry run, approval and drift repair |
| Octavia | Native layer-4 balancer on the host, no amphora VM |
| Horizon | The web UI, with browser consoles |
| Masakari, Watcher | HA failover and DRS in the controller |
| Ceilometer, Aodh | Metrics history, alarms with scaling actions, Prometheus and OTLP |
| MariaDB, RabbitMQ | SQLite or PostgreSQL; in-memory task bus, NATS optional |

**Gaps, stated plainly:** thousands of tenants and Neutron-grade SDN breadth are out of scope; the OpenStack client tools and
ecosystem projects do not work against Machina; there is no OpenStack importer, and moving a guest as a disk image has not been run end to end on a real OpenStack
cloud. Which features have run on a real host and which are unit-tested only: [claims.md](docs/claims.md). The full map, with how to move a workload: [Replacing OpenStack](docs/migration/from-openstack.md).

### Machina vs OpenStack, side by side

![Machina vs OpenStack — same private-cloud primitives, a fraction of the moving parts](docs/ux/readme-vs-openstack.jpg)

| | **Machina** | **OpenStack** (typical IaaS) |
|---|---|---|
| Services to run | **4** Rust services | 9+ services (Keystone, Nova, Neutron, Glance, Cinder, Placement, Horizon, Heat, Octavia) |
| Backing infrastructure | Embedded SQLite or PostgreSQL; optional NATS | MariaDB/Galera, RabbitMQ, Memcached |
| Install | `./machinactl deploy` | Kolla-Ansible / OpenStack-Ansible project |
| Smallest useful footprint | A single KVM host | A multi-node control plane |
| Flavors, images, volumes, SGs, stacks, LBs | Yes, in [Fleet Cloud](docs/customer/pages/fleet-cloud/fleet-cloud.md) | Yes, across six projects |
| Load balancer data plane | On the host, no amphora VM: iptables rules for Fleet Cloud, eBPF Maglev for Kubernetes services | Amphora VMs (Octavia) |
| Network datapath | Native eBPF, lease-gated enforcement | Neutron agents + OVS/OVN |
| Browser consoles | Built into the daemon | noVNC/SPICE proxy services |
| HA failover and DRS | Built in ([controller HA](docs/controller-ha.md)) | Masakari + Watcher (separate projects) |
| AI operations | Zyra AI, approval-gated | Not included |
| Idle VMs | [Scale to zero](docs/fleet-cloud-features.md#scale-to-zero): sleep when idle, wake on the first packet | Shelve and unshelve by hand |
| Copies and rollback | [Time travel](docs/fleet-cloud-features.md#time-travel): live fork (optionally with RAM), restore points, rewind in place | Snapshot uploads a full image to Glance; rebuild from it |
| Orchestration | [Stacks](docs/fleet-cloud-features.md#stacks-you-describe) drafted from a description, dry-run with cost and policy replay, approved, then kept in sync every minute | Heat: hand-written HOT templates; no drift repair |
| Autoscaling and sizing | [Autopilot](docs/fleet-cloud-features.md#autopilot-capacity): seasonal forecast pre-scaling, load-balancer drain, sleep on scale-in, history-based rightsizing and host consolidation with approval and undo | Senlin/Heat alarms react after the fact; Watcher is a separate service; resizes by hand |
| Resilience testing | [Game days](docs/fleet-cloud-features.md#game-days): leased network, disk and crash faults with health-probe abort and a report | Not included; a separate chaos tool |
| Spare capacity | [Preemptible instances](docs/fleet-cloud-features.md#preemptible-instances): saved to disk under memory pressure by priority and restored automatically | Not included; spot capacity deletes the instance |
| Containers and Kubernetes | Podman, KubeVirt | Zun / Magnum (separate projects) |
| **Choose OpenStack when** | | You run thousands of tenants, need Neutron-grade SDN breadth, or depend on its ecosystem |

Machina targets the fleets you own: a lab, a branch, a sovereign region, a VMware exit. It deliberately trades OpenStack's hyperscale multi-tenancy for a cloud one person can install, understand and upgrade.

---

## See it live

Captured from a real deployment on Ubuntu 26.04 in the dark theme, not mockups.

![Machina Mission Control, dark theme](docs/ux/machina-dashboard-dark.png)

### Every VM operation, in one place

Create, clone, snapshot, back up and migrate. Cloud-init, GPU and PCI passthrough, golden images built with Packer (Linux, plus Windows 10/11 through dockur when enabled), networks, storage pools and nwfilters. [VM guide →](docs/customer/pages/core/vms.md)

![Create a VM: guided wizard with a live summary](docs/ux/machina-create-dark.png)

### Consoles in the browser, no gateway to deploy

noVNC, SPICE, serial and SSH are proxied by `machina-daemon` itself, behind the same RBAC and audit log as everything else. VM detail leads with a live console preview. [Console architecture →](docs/consolehub-architecture.md)

### A fleet, not a host

Add hypervisors with a gRPC agent over TLS. The controller keeps desired state, fails VMs over when a host dies, balances load with DRS and live-migrates between hosts. [Controller HA and DRS →](docs/controller-ha.md)

### Fleet Cloud: self-service like a public cloud

Flavors, images, instances, volumes and snapshots, security groups, keypairs, floating IPs, server groups, Heat-style stacks, projects and load balancers, all native controller APIs. Beyond that, idle instances can scale to zero and wake on traffic. EC2-style semantics on top: tags and ids, instance types, user data, enforced security groups, Elastic IPs and a NAT gateway, an instance metadata service, alarms with scaling actions, project-scoped API keys, and an EC2-compatible endpoint (`aws`/boto3 work with an endpoint override). [EC2 semantics →](docs/cloud-ec2-semantics.md) · [EC2 API →](docs/cloud-ec2-api.md) [Fleet Cloud →](docs/customer/pages/fleet-cloud/fleet-cloud.md) · [Features OpenStack doesn't have →](docs/fleet-cloud-features.md)

![Fleet Cloud](docs/ux/machina-fleet-cloud-dark.png)

### Zyra AI: an operator that asks first

Autonomous diagnostics across the fleet, incident correlation, rightsizing and natural-language operations, with bring-your-own LLM providers (keys encrypted at rest) and an approval queue in front of every change. [Zyra AI →](docs/customer/pages/platform-security/platform-zyra.md)

![Zyra AI](docs/ux/machina-zyra-dark.png)

### Networking and security, in the kernel

Machina ships its own eBPF datapath instead of bolting on Cilium, Tetragon or a separate firewall agent. One root service, `machina-bpfd`, provides:

- **Load balancing**: Maglev service LB for Kubernetes (socket-level, NodePort at TC or XDP, DSR) and QUIC-LB at XDP.
- **Kubernetes CNI** (opt-in): `machina-cni` replaces flannel and kube-proxy where you choose it, with NetworkPolicy and Cilium policy migration. It never takes over an existing CNI.
- **VM network policy**: which VM may talk to which, ingress and egress, written as CiliumNetworkPolicy YAML and enforced on each VM tap without Cilium or Envoy. This covers L3/L4, DNS names (`toFQDNs`), L7 (HTTP, gRPC, Kafka, TLS SNI, DNS), TLS interception with header rewrites, CIDR groups and mutual authentication (mTLS between hosts). Comes with policy trace and Hubble-style flows in the UI and in `machinactl netpol` / `machinactl flow`, plus flow history (service map, learn, replay, L7 metrics), quarantine, just-in-time access, lateral-movement alerts, DNS threat feeds, plain-English policies, project isolation, egress allowlists and IPs (added to the host and announced by Machina), an encrypted WireGuard overlay that keeps VM identity across hosts, VM addresses found without a guest agent (neighbour tables and tap traffic), and segmentation evidence export.
- **Protection**: XDP DDoS shield, emergency node isolation, VM edge isolation and rate limits, a QEMU sandbox and a BPF-LSM guard around the VMM.
- **Visibility**: flows, DNS, L7 (HTTP, TLS SNI, gRPC, Redis, PostgreSQL, MySQL, Kafka), JA3/JA4 fingerprints, network-change audit and per-VM runtime histograms.
- **Inside guests**: per-container network and LSM policy through GuestKit, from the VM's **Guest policy** tab.

Everything that can drop traffic starts in observe mode and enforces only under a time-boxed lease that the kernel honours on its own; nothing is persisted. [Native eBPF →](docs/ebpf/README.md)

![Native eBPF on a live host: machina-bpfd datapath loaded](docs/ux/machina-native-ebpf-dark.png)

![Security Center: Zeus firewall, SOC, threat hunting, runtime enforcement](docs/ux/machina-security-dark.png)

### And the rest of the platform

- **Identity and access**: PAM, OIDC, SAML and [LDAP](docs/ldap-auth.md) sign-in, role-based access, a full audit trail. [Admin guide →](docs/handbook/admin-configuration.md)
- **Containers**: local Podman/Docker containers and Podman pods next to your VMs. [Containers →](docs/customer/pages/infrastructure/containers.md)
- **Kubernetes**: KubeVirt inventory and a documented migration path. [KubeVirt →](docs/kubevirt-migration.md)
- **FluxVM**: QEMU, Cloud Hypervisor, Firecracker and flux-vm VMs from FluxVM next to libvirt VMs, with snapshots, backups, hot-add, install ISOs, live migration and fleet HA. [FluxVM →](docs/fluxvm.md)
- **Observability**: Prometheus metrics, OTLP export, PSI/cgroup pressure, alerts and webhooks. [Observability →](docs/guides/observability.md)
- **Storage**: Atlas integration puts VM disks on Ceph RBD, NFS or ZFS volumes with snapshot, backup and restore. [Atlas →](docs/atlas-storage.md)
- **Automation**: [Terraform provider](terraform/machina/README.md), [TypeScript SDK](sdk/typescript/README.md), OpenAPI spec, `machinactl`.

---

## How it fits together

![Four Rust services, one private cloud](docs/ux/readme-architecture.jpg)

| Component | Port | Role |
|---|---|---|
| `machina-daemon` | `:5092` | Single-host API, auth/RBAC, console proxies, web UI |
| `machina-controller` | `:5093` | Fleet, HA, DRS, Fleet Cloud, Zyra AI (SQLite or PostgreSQL) |
| `machina-agent` | `:50051`, `:50052` | Per-host gRPC agent (TLS) for libvirt and eBPF ops; `:50052` is its console proxy |
| `machina-bpfd` | unix socket | Root eBPF datapath, telemetry and enforcement (`/api/v1/bpf/*`) |
| `machina-cni` | — | Opt-in Kubernetes CNI plugin and node agent |
| `machina-scx` | — | Optional sched_ext VM scheduler (kernel 6.12+) |

![Deploy a fleet: agents join, the controller fails a VM over](docs/social/anim/deploy-fleet.svg)

The daemon alone is a complete single-host manager. Add the controller and an agent per host for a fleet; `machina-bpfd` runs on every host that should get the eBPF datapath.

---

## Database: SQLite or PostgreSQL

![SQLite or PostgreSQL: start embedded, grow into a shared database](docs/ux/readme-database.jpg)

The controller keeps its state in one database. Both backends come from one source tree and are chosen when the controller is built.

| | SQLite (default) | PostgreSQL |
|---|---|---|
| Setup | None; one file, created and migrated on first start | `machinactl db setup pod\|package\|external` |
| Fits | 1 or 2 hosts, labs, evaluations | Hundreds of machines |
| Controllers | One | Several on different hosts; one leader, hand-over inside the lease |
| Way back | n/a | `machinactl db setup sqlite` (the SQLite file is never touched) |

![Move the controller from SQLite to PostgreSQL with one command](docs/social/anim/deploy-postgres.svg)

```bash
sudo machinactl db setup pod --migrate     # managed PostgreSQL 16 in Podman, copies your SQLite data
sudo machinactl db status                  # backend, login, size, migrations
sudo machinactl db backup                  # pg_dump, or an online copy on SQLite
# or at install time: scripts/install-platform.sh --database pod|package|external|sqlite
```

Every schema change ships as two migrations, and CI prepares every SQL statement against PostgreSQL. **Status:** the PostgreSQL build passes the controller's whole test suite on PostgreSQL 16, and an isolated PostgreSQL-backed controller was run live (two controllers on one database elect one leader and hand over within the lease; 500 machines list in under a second), and a copy of the lab host's real database loaded with identical row counts. Not done yet: a PostgreSQL controller managing real hosts. Until a site has run on it, SQLite stays the safe default ([claim C24](docs/claims.md), [guide](docs/guides/database.md)).

---

## EC2-compatible API

![EC2-compatible API: stock AWS clients to the controller to your hosts](docs/ux/readme-ec2.jpg)

![Launch instances with the aws CLI against Machina](docs/social/anim/ec2-launch.svg)

The controller answers four AWS query services (EC2, Auto Scaling, ELBv2 and CloudWatch-style alarms) with SigV4-signed requests, so
`aws`, boto3 and Terraform's `aws` provider work with an endpoint override. It is a compatibility layer over Machina's own objects, **not AWS**: 288 actions are
mapped, each one applied, recorded as a stored plan, or refused by name, never silently dropped. The per-action status table is generated from the code.

```bash
# an admin creates an access key through the daemon (the secret is shown once)
curl -sk -c jar -X POST https://HOST:5092/api/v1/auth/login -H 'content-type: application/json' -d '{"username":"USER","password":"PASS"}'
curl -sk -b jar -X POST https://HOST:5092/api/v1/platform/controller/api/v1/ec2/access-keys \
     -H 'content-type: application/json' -d '{"description":"ci"}'

# the EC2 endpoint is plain http on the controller's loopback port 5093; run this on the host, or tunnel:
#   ssh -N -L 15093:127.0.0.1:5093 USER@HOST     (then use http://127.0.0.1:15093)
export AWS_ACCESS_KEY_ID=MCAK… AWS_SECRET_ACCESS_KEY=… AWS_DEFAULT_REGION=machina
EC2=http://127.0.0.1:5093/ec2
aws --endpoint-url $EC2 ec2 run-instances --image-id <ImageId> --count 3
aws --endpoint-url $EC2 ec2 describe-instances
```

```python
import boto3   # same endpoint, same keys (run on the host or through the tunnel)
ec2 = boto3.client("ec2", endpoint_url="http://127.0.0.1:5093/ec2", region_name="machina")
print([i["InstanceId"] for r in ec2.describe_instances()["Reservations"] for i in r["Instances"]])
```

**Status:** implemented and unit-tested; not every action has been exercised with a stock client ([what has been run](docs/cloud-ec2-api.md#what-has-been-run-with-a-real-client)). [API and action table →](docs/cloud-ec2-api.md) · [Clients and Terraform →](docs/cloud-ec2-clients.md) · [Semantics →](docs/cloud-ec2-semantics.md)

---

## Quickstart

On any Linux host with KVM (Ubuntu, Debian, Fedora, RHEL/Alma/Rocky, openSUSE, Arch):

```bash
git clone https://github.com/zyvorai/zyvor-machina.git machina && cd machina
./machinactl deploy        # deps · build · install · start · verify
# open https://<host>:5092 and sign in with a local (PAM) account
# controller state is embedded SQLite; for PostgreSQL see "Database" above
# (package installs create an admin user instead: sudo machinactl show-login prints it)
```

From your laptop to a remote host (sources are rsync'd and built on the server; nothing compiles locally):

```bash
./scripts/deploy-remote.sh user@host                  # build + install daemon and web UI on the server
./scripts/deploy-remote.sh user@host --platform       # plus controller and agent
./scripts/deploy-remote.sh user@host --remote-build   # compile only, no install (fast build check)
```

### Requirements

- 64-bit Linux (x86_64 or aarch64) with VT-x/AMD-V, so `/dev/kvm` exists, and libvirt.
- Ubuntu, Debian, Fedora, RHEL/Alma/Rocky, openSUSE or Arch.
- Native eBPF: kernel with BTF and cgroup v2; 6.6+ for TCX attach, BPF-LSM (`lsm=...,bpf`) for VMM guard enforcement, 6.12+ for the sched_ext scheduler. `GET /api/v1/bpf/status` shows what your kernel supports.
- Ports: `5092` (daemon, web UI), `5093` (controller), `50051`/`50052` (agent). [Full port list →](docs/handbook/README.md#ports)
- Any current browser; consoles need no plugins.

| Next step | Where |
|---|---|
| One copy-paste path: controller, node, VM, backup | [Quickstart](docs/QUICKSTART.md) |
| First login and workflows | [Getting started](docs/customer/getting-started.md) |
| Every screen, explained | [Page-by-page guides](docs/customer/pages/README.md) |
| Ports, auth, TLS, config | [Admin configuration](docs/handbook/admin-configuration.md) |
| Production pilot checklist | [Customer site readiness](docs/CUSTOMER_SITE_READINESS.md) |
| Contributor setup | [Engineering onboarding](docs/ENGINEERING_ONBOARDING.md) |
| Everything else | [Docs index](docs/README.md) · [Website](https://zyvorai.github.io/zyvor-machina/) |

---

## Maturity

| Area | Status |
|---|---|
| VMs, storage, networks, consoles, auth, audit | Stable |
| Fleet, HA with fencing, DRS, live migration | Stable |
| Fleet Cloud | Stable |
| eBPF observability | Preview |
| eBPF enforcement | Preview, lease-gated |
| `machina-cni` | Preview, opt-in |
| QUIC-LB, AF_XDP, sched_ext, guest policy | Opt-in |
| Zyra AI | Stable, approval-gated |
| Atlas storage integration | Opt-in (`ATLAS_ENABLED=1`) |

---

## Develop

The Rust workspace needs Linux libvirt headers; build on Linux (or use `deploy-remote.sh --remote-build` from a Mac). The web UI builds anywhere.

```bash
make build && make test && make lint       # Rust workspace (Linux)
make bpf-deps                              # eBPF toolchain: nightly + bpf-linker
sudo make bpf-test                         # eBPF netns smoke (veths only)
cd web && npm install && npm run dev       # UI on :3000, proxied to a daemon on :5092
cd web && npm test && npm run build        # vitest + typecheck + bundle
```

Start with [Engineering onboarding](docs/ENGINEERING_ONBOARDING.md); architecture and conventions are in [CLAUDE.md](CLAUDE.md).

---

## Part of the Zyvor stack

| Product | Role |
|---|---|
| **Machina** | Private cloud on KVM: VMs, fleet, Fleet Cloud, native eBPF, Zyra AI |
| **Atlas** | Storage control plane (Ceph/NFS/ZFS) for VM disks, snapshots, backups |
| **GuestKit** | In-guest agent, offline inspection, per-container eBPF policy |
| **hyper2kvm** | VM migration into KVM |

→ [zyvor.dev](https://zyvor.dev)

---

## License

Machina is source-available under the **[Zyvor Production License v1.0](LICENSE)** (SPDX `LicenseRef-Zyvor-Production-1.0`).

- **Free** for evaluation, development, testing, research, education, homelabs and all other non-production use.
- **Production use** requires an annual enterprise subscription. Plans, support levels and terms: [SUBSCRIPTION-MODEL.md](docs/SUBSCRIPTION-MODEL.md) · [licensing model](docs/legal/LICENSING-MODEL.md).

Contributions are welcome under the same license; see [CONTRIBUTING.md](CONTRIBUTING.md). Report vulnerabilities privately per [SECURITY.md](SECURITY.md).

---

<div align="center">

### Ready to run your own cloud?

[![Book a demo](https://img.shields.io/badge/Book_a_demo-0071e3?style=for-the-badge)](https://zyvor.dev/schedule?utm_source=github&utm_medium=machina&utm_campaign=readme_footer)
[![30-day PoC](https://img.shields.io/badge/Start_a_30--day_PoC-000000?style=for-the-badge)](https://zyvor.dev/poc?utm_source=github&utm_medium=machina&utm_campaign=readme_footer)
[![Star on GitHub](https://img.shields.io/github/stars/zyvorai/zyvor-machina?style=for-the-badge&logo=github&label=Star&color=2997ff)](https://github.com/zyvorai/zyvor-machina)

</div>
