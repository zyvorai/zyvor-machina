# Machina — Complete page index

Every primary navigable dashboard route.

_Generated: 2026-11-18 · 118 routes_

Regenerate: `node scripts/customer-docs/generate-page-index.mjs`

## Core

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Dashboard | `/` | Host dashboard — VM inventory pulse, Vessel container shortcuts when Podman/Docker is connected, and quick links. | [Open](pages/core/home.md) |
| Virtual Machines | `/vms` | Virtual machine inventory for this libvirt host. | [Open](pages/core/vms.md) |
| Create VM | `/create` | Create a new libvirt VM. | [Open](pages/core/create.md) |
| Import VM | `/import` | Import VM — Machina Core page at `/import`. | [Open](pages/core/import.md) |
| Sprites | `/sprites` | Instant, disposable sandbox VMs — boot on libvirt/QEMU, Cloud Hypervisor, or Firecracker, TTL-reaped automatically, no persistent state. | [Open](pages/core/sprites.md) |
| Fleet | `/fleet` | Fleet — Machina Core page at `/fleet`. | [Open](pages/core/fleet.md) |
| Host SSH | `/host-ssh` | Host SSH — Machina Core page at `/host-ssh`. | [Open](pages/core/host-ssh.md) |
| Capabilities | `/capabilities` | Capabilities — Machina Core page at `/capabilities`. | [Open](pages/core/capabilities.md) |
| Node Devices | `/devices` | Node Devices — Machina Core page at `/devices`. | [Open](pages/core/devices.md) |
| Services | `/services` | Services — Machina Core page at `/services`. | [Open](pages/core/services.md) |

## Platform

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Mission Control | `/platform` | Mission Control — multi-host platform overview. | [Open](pages/platform/platform.md) |
| Virtual Machines | `/platform/vms` | Fleet VM finder across enrolled hosts. | [Open](pages/platform/platform-vms.md) |
| Hosts | `/platform/hosts` | Hosts — Machina Platform page at `/platform/hosts`. | [Open](pages/platform/platform-hosts.md) |
| Machine Finder | `/platform/hosts/finder` | Machine Finder: opens the fleet VM list in its topology lens (/platform/vms?lens=topology) to find any VM by host, network or name. | [Open](pages/platform/platform-hosts-finder.md) |
| Applications | `/platform/applications` | Applications — Machina Platform page at `/platform/applications`. | [Open](pages/platform/platform-applications.md) |
| Launchpad (Mission Control) | `/platform/launchpad` | Launchpad tiles live on Mission Control (`/platform`) — there is no separate `/platform/launchpad` route. | [Open](pages/platform/platform-launchpad.md) |
| Datacenter View | `/platform/datacenter` | Datacenter View — Machina Platform page at `/platform/datacenter`. | [Open](pages/platform/platform-datacenter.md) |
| Disk Utility | `/platform/storage` | Disk Utility — Machina Platform page at `/platform/storage`. | [Open](pages/platform/platform-storage.md) |
| Storage (Atlas) | `/platform/storage-atlas` | Storage (Atlas) — Machina Platform page at `/platform/storage-atlas`. | [Open](pages/platform/platform-storage-atlas.md) |
| Storage Tiers | `/platform/storage-tiers` | Storage Tiers — Machina Platform page at `/platform/storage-tiers`. | [Open](pages/platform/platform-storage-tiers.md) |
| vGPU Partitions | `/platform/vgpu` | vGPU Partitions — split a GPU into mediated-device partitions, give each to a VM, and see which VMs are pinned to a host by one. Page at `/platform/vgpu`. | [Open](pages/platform/platform-vgpu.md) |
| Networks | `/platform/networks` | Networks — Machina Platform page at `/platform/networks`. | [Open](pages/platform/platform-networks.md) |
| Images & ISOs | `/platform/content` | Images & ISOs — Machina Platform page at `/platform/content`. | [Open](pages/platform/platform-content.md) |
| Templates | `/platform/templates` | Templates — Machina Platform page at `/platform/templates`. | [Open](pages/platform/platform-templates.md) |
| Cloud-Init Studio | `/platform/cloud-init` | Cloud-Init Studio — Machina Platform page at `/platform/cloud-init`. | [Open](pages/platform/platform-cloud-init.md) |
| Create ISO | `/platform/create-iso` | Create ISO — Machina Platform page at `/platform/create-iso`. | [Open](pages/platform/platform-create-iso.md) |
| Bare Metal | `/platform/baremetal` | Bare Metal — Machina Platform page at `/platform/baremetal`. | [Open](pages/platform/platform-baremetal.md) |
| Migration Assistant | `/platform/migration` | Migration Assistant — Machina Platform page at `/platform/migration`. | [Open](pages/platform/platform-migration.md) |
| VM Builder | `/platform/vm-builder` | VM Builder — Machina Platform page at `/platform/vm-builder`. | [Open](pages/platform/platform-vm-builder.md) |
| Advanced Create | `/platform/create-advanced` | Advanced Create — Machina Platform page at `/platform/create-advanced`. | [Open](pages/platform/platform-create-advanced.md) |
| Blueprints | `/platform/blueprints` | Blueprints — Machina Platform page at `/platform/blueprints`. | [Open](pages/platform/platform-blueprints.md) |
| Tasks | `/platform/tasks` | Tasks — Machina Platform page at `/platform/tasks`. | [Open](pages/platform/platform-tasks.md) |
| Network Canvas | `/platform/network-canvas` | Network Canvas — Machina Platform page at `/platform/network-canvas`. | [Open](pages/platform/platform-network-canvas.md) |
| Security Operations | `/platform/soc` | Security Operations — Machina Platform page at `/platform/soc`. | [Open](pages/platform/platform-soc.md) |
| Policy | `/platform/policy` | Policy — Machina Platform page at `/platform/policy`. | [Open](pages/platform/platform-policy.md) |
| Webhooks | `/platform/webhooks` | Webhooks — Machina Platform page at `/platform/webhooks`. | [Open](pages/platform/platform-webhooks.md) |
| Backup & Restore | `/platform/backups` | Backup & Restore — Machina Platform page at `/platform/backups`. | [Open](pages/platform/platform-backups.md) |
| Fleet Snapshots | `/platform/fleet-snapshots` | Fleet Snapshots — Machina Platform page at `/platform/fleet-snapshots`. | [Open](pages/platform/platform-fleet-snapshots.md) |
| Backup Replication | `/platform/backup-replication` | Backup Replication — copy completed backups to an S3-compatible store at another site, see which copies are verified, and fetch one back onto a host. Page at `/platform/backup-replication`. | [Open](pages/platform/platform-backup-replication.md) |
| Failure Domains | `/platform/failure-domains` | Failure Domains — racks defined by host site and rack labels, and whether HA could re-home each rack if it were lost. Page at `/platform/failure-domains`. | [Open](pages/platform/platform-failure-domains.md) |
| Kubernetes Clusters | `/platform/k8s-clusters` | Kubernetes Clusters — create, scale, upgrade and delete k3s clusters across enrolled hosts (opt-in on the controller and each host). Page at `/platform/k8s-clusters`. | [Open](pages/platform/platform-k8s-clusters.md) |
| Desktop Pools | `/platform/desktop-pools` | Desktop Pools — clone a template VM into numbered desktops, check one out per user and recycle it on release. Page at `/platform/desktop-pools`. | [Open](pages/platform/platform-desktop-pools.md) |
| Migration Waves | `/platform/migration-waves` | Migration Waves — plan a batch of VMs for Transiva, start them a few at a time, follow each job and record cutover. Page at `/platform/migration-waves`. | [Open](pages/platform/platform-migration-waves.md) |
| Disaster Recovery | `/platform/placement` | Disaster Recovery — Machina Platform page at `/platform/placement`. | [Open](pages/platform/platform-placement.md) |
| High Availability | `/platform/ha` | High Availability — Machina Platform page at `/platform/ha`. | [Open](pages/platform/platform-ha.md) |
| Upgrade Matrix | `/platform/upgrade` | Upgrade Matrix — Machina Platform page at `/platform/upgrade`. | [Open](pages/platform/platform-upgrade.md) |
| Maintenance | `/platform/maintenance` | Maintenance — Machina Platform page at `/platform/maintenance`. | [Open](pages/platform/platform-maintenance.md) |
| Recommendations | `/platform/recommendations` | Recommendations — Machina Platform page at `/platform/recommendations`. | [Open](pages/platform/platform-recommendations.md) |
| Alerts | `/platform/notifications` | Alerts — Machina Platform page at `/platform/notifications`. | [Open](pages/platform/platform-notifications.md) |
| Alert Rules | `/platform/alert-rules` | Threshold alert rules on VM CPU and memory: notify (via Alerts / webhooks) when a metric crosses a bound. | [Open](pages/platform/platform-alert-rules.md) |
| Scheduled Jobs | `/platform/scheduled-jobs` | Recurring controller operations from a whitelist, for example periodic host inventory refresh. | [Open](pages/platform/platform-scheduled-jobs.md) |
| Observability | `/platform/observability` | Observability — Machina Platform page at `/platform/observability`. | [Open](pages/platform/platform-observability.md) |
| Activity Monitor | `/platform/activity` | Activity Monitor — Machina Platform page at `/platform/activity`. | [Open](pages/platform/platform-activity.md) |
| Topology | `/platform/topology` | Topology — Machina Platform page at `/platform/topology`. | [Open](pages/platform/platform-topology.md) |
| Reports | `/platform/reports` | Reports — Machina Platform page at `/platform/reports`. | [Open](pages/platform/platform-reports.md) |
| GPU Command Center | `/platform/gpu` | GPU Command Center — Machina Platform page at `/platform/gpu`. | [Open](pages/platform/platform-gpu.md) |
| Settings | `/platform/settings` | Settings — Machina Platform page at `/platform/settings`. | [Open](pages/platform/platform-settings.md) |
| Users & Groups | `/platform/users` | Users & Groups — Machina Platform page at `/platform/users`. | [Open](pages/platform/platform-users.md) |
| Stage Manager | `/platform/projects` | Stage Manager — Machina Platform page at `/platform/projects`. | [Open](pages/platform/platform-projects.md) |
| Add Host | `/platform/enroll` | Add Host — Machina Platform page at `/platform/enroll`. | [Open](pages/platform/platform-enroll.md) |
| API Keys | `/platform/api-keys` | API Keys — Machina Platform page at `/platform/api-keys`. | [Open](pages/platform/platform-api-keys.md) |
| Marketplace | `/platform/marketplace` | Marketplace — Machina Platform page at `/platform/marketplace`. | [Open](pages/platform/platform-marketplace.md) |
| AI Providers | `/platform/ai-providers` | AI Providers — Machina Platform page at `/platform/ai-providers`. | [Open](pages/platform/platform-ai-providers.md) |
| Enterprise Features | `/platform/enterprise` | Enterprise Features — Machina Platform page at `/platform/enterprise`. | [Open](pages/platform/platform-enterprise.md) |
| Developer Hub | `/platform/developer` | Developer Hub — Machina Platform page at `/platform/developer`. | [Open](pages/platform/platform-developer.md) |
| Support | `/platform/support` | Support — Machina Platform page at `/platform/support`. | [Open](pages/platform/platform-support.md) |
| Event Log | `/platform/events` | Event Log — Machina Platform page at `/platform/events`. | [Open](pages/platform/platform-events.md) |
| Settings | `/settings` | Daemon and UI settings, including Fleet Cloud wiring. | [Open](pages/platform/settings.md) |
| API Docs | `/api-docs` | API Docs — Platform surface. | [Open](pages/platform/api-docs.md) |

## Platform / Security

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Zyra AI | `/platform/zyra` | Zyra AI assistant for Machina operations. | [Open](pages/platform-security/platform-zyra.md) |
| Configure Zyra | `/platform/zyra/configure` | Configure Zyra — Machina Platform / Security page at `/platform/zyra/configure`. | [Open](pages/platform-security/platform-zyra-configure.md) |
| Zeus Security | `/platform/zeus/security` | Zeus Security — Machina Platform / Security page at `/platform/zeus/security`. | [Open](pages/platform-security/platform-zeus-security.md) |
| Native eBPF | `/platform/zyra/security/native-bpf` | Native eBPF: per-host machina-bpfd status, observe/enforce mode under a lease, deny/allow policies, flows, DNS, L7, load balancing, CNI and guest policy across 22 tabs. | [Open](pages/platform-security/platform-zyra-security-native-bpf.md) |
| Incident Commander | `/platform/zyra/incidents` | Incident Commander — Machina Platform / Security page at `/platform/zyra/incidents`. | [Open](pages/platform-security/platform-zyra-incidents.md) |
| Approvals | `/platform/zyra/approvals` | Approvals — Machina Platform / Security page at `/platform/zyra/approvals`. | [Open](pages/platform-security/platform-zyra-approvals.md) |
| Rightsizing | `/platform/zyra/rightsizing` | Rightsizing — Machina Platform / Security page at `/platform/zyra/rightsizing`. | [Open](pages/platform-security/platform-zyra-rightsizing.md) |
| VM Network Policies | `/platform/zyra/security/network-policies` |  | [Open](pages/platform-security/platform-zyra-security-network-policies.md) |

## Infrastructure

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Storage Pools | `/storage` | Storage Pools — Machina Infrastructure page at `/storage`. | [Open](pages/infrastructure/storage.md) |
| Disk Images | `/disk-images` | Disk Images — Machina Infrastructure page at `/disk-images`. | [Open](pages/infrastructure/disk-images.md) |
| Snapshots | `/snapshots` | Snapshots — Machina Infrastructure page at `/snapshots`. | [Open](pages/infrastructure/snapshots.md) |
| Snapshot Backups | `/backups` | Snapshot Backups — Machina Infrastructure page at `/backups`. | [Open](pages/infrastructure/backups.md) |
| Networks | `/networks` | Networks — Machina Infrastructure page at `/networks`. | [Open](pages/infrastructure/networks.md) |
| Network Filters | `/nwfilters` | Network Filters — Machina Infrastructure page at `/nwfilters`. | [Open](pages/infrastructure/nwfilters.md) |
| Host Networking | `/host-networking` | Host Networking — Machina Infrastructure page at `/host-networking`. | [Open](pages/infrastructure/host-networking.md) |
| Secrets | `/secrets` | Secrets — Machina Infrastructure page at `/secrets`. | [Open](pages/infrastructure/secrets.md) |
| Containers | `/containers` | Local Podman/Docker containers via Vessel — list, create, lifecycle, and live stats on this host. | [Open](pages/infrastructure/containers.md) |
| Container Pods | `/containers/pods` | Podman pods via Vessel — create and manage shared-namespace container groups (not Kubernetes pods). | [Open](pages/infrastructure/containers-pods.md) |

## Kubernetes

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Kubernetes | `/k8s` | Kubernetes — Machina Kubernetes page at `/k8s`. | [Open](pages/kubernetes/k8s.md) |
| K8s Workloads | `/k8s/workloads` | K8s Workloads — Machina Kubernetes page at `/k8s/workloads`. | [Open](pages/kubernetes/k8s-workloads.md) |
| Kata + Cloud Hypervisor | `/k8s/kata` | Kata + Cloud Hypervisor — Machina Kubernetes page at `/k8s/kata`. | [Open](pages/kubernetes/k8s-kata.md) |

## Fleet Cloud

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Overview | `/fleet-cloud` | Overview — Machina Fleet Cloud page at `/fleet-cloud`. | [Open](pages/fleet-cloud/fleet-cloud.md) |
| Instances | `/fleet-cloud/instances` | Instances — Machina Fleet Cloud page at `/fleet-cloud/instances`. | [Open](pages/fleet-cloud/fleet-cloud-instances.md) |
| Create Instance | `/fleet-cloud/create` | Create Instance — Machina Fleet Cloud page at `/fleet-cloud/create`. | [Open](pages/fleet-cloud/fleet-cloud-create.md) |
| Server Groups | `/fleet-cloud/server-groups` | Server Groups — Machina Fleet Cloud page at `/fleet-cloud/server-groups`. | [Open](pages/fleet-cloud/fleet-cloud-server-groups.md) |
| Keypairs | `/fleet-cloud/keypairs` | Keypairs — Machina Fleet Cloud page at `/fleet-cloud/keypairs`. | [Open](pages/fleet-cloud/fleet-cloud-keypairs.md) |
| Flavors | `/fleet-cloud/flavors` | Flavors — Machina Fleet Cloud page at `/fleet-cloud/flavors`. | [Open](pages/fleet-cloud/fleet-cloud-flavors.md) |
| VM QoS | `/fleet-cloud/qos` | VM QoS — Machina Fleet Cloud page at `/fleet-cloud/qos`. | [Open](pages/fleet-cloud/fleet-cloud-qos.md) |
| Volumes | `/fleet-cloud/volumes` | Volumes — Machina Fleet Cloud page at `/fleet-cloud/volumes`. | [Open](pages/fleet-cloud/fleet-cloud-volumes.md) |
| Volume Snapshots | `/fleet-cloud/volume-snapshots` | Volume Snapshots — Machina Fleet Cloud page at `/fleet-cloud/volume-snapshots`. | [Open](pages/fleet-cloud/fleet-cloud-volume-snapshots.md) |
| Images | `/fleet-cloud/images` | Images — Machina Fleet Cloud page at `/fleet-cloud/images`. | [Open](pages/fleet-cloud/fleet-cloud-images.md) |
| Networking | `/fleet-cloud/networking` | Networking — Machina Fleet Cloud page at `/fleet-cloud/networking`. | [Open](pages/fleet-cloud/fleet-cloud-networking.md) |
| Security Groups | `/fleet-cloud/security-groups` | Security Groups — Machina Fleet Cloud page at `/fleet-cloud/security-groups`. | [Open](pages/fleet-cloud/fleet-cloud-security-groups.md) |
| Floating IPs | `/fleet-cloud/floating-ips` | Floating IPs — Machina Fleet Cloud page at `/fleet-cloud/floating-ips`. | [Open](pages/fleet-cloud/fleet-cloud-floating-ips.md) |
| Load Balancers | `/fleet-cloud/load-balancers` | Load Balancers — Machina Fleet Cloud page at `/fleet-cloud/load-balancers`. | [Open](pages/fleet-cloud/fleet-cloud-load-balancers.md) |
| Network Topology | `/fleet-cloud/topology` | Network Topology — Machina Fleet Cloud page at `/fleet-cloud/topology`. | [Open](pages/fleet-cloud/fleet-cloud-topology.md) |
| VPCs & elastic compute | `/fleet-cloud/vpcs` | Isolated subnets and desired-capacity instance groups. | [Open](pages/fleet-cloud/fleet-cloud-vpcs.md) |
| Heat Orchestration | `/fleet-cloud/heat` | Heat Orchestration — Machina Fleet Cloud page at `/fleet-cloud/heat`. | [Open](pages/fleet-cloud/fleet-cloud-heat.md) |
| Identity | `/fleet-cloud/identity` | Identity — Machina Fleet Cloud page at `/fleet-cloud/identity`. | [Open](pages/fleet-cloud/fleet-cloud-identity.md) |
| Autopilot | `/fleet-cloud/autopilot` |  | [Open](pages/fleet-cloud/fleet-cloud-autopilot.md) |
| Game days | `/fleet-cloud/chaos` |  | [Open](pages/fleet-cloud/fleet-cloud-chaos.md) |
| Preemptible | `/fleet-cloud/preemptible` |  | [Open](pages/fleet-cloud/fleet-cloud-preemptible.md) |

## Monitoring

| Page | Route | Purpose | Guide |
|------|-------|---------|-------|
| Host Overview | `/node` | Host Overview — Machina Monitoring page at `/node`. | [Open](pages/monitoring/node.md) |
| Live Metrics | `/events` | Live Metrics — Machina Monitoring page at `/events`. | [Open](pages/monitoring/events.md) |
| System Check | `/system-check` | System Check — Machina Monitoring page at `/system-check`. | [Open](pages/monitoring/system-check.md) |
| Daemon Jobs | `/jobs` | Daemon Jobs — Machina Monitoring page at `/jobs`. | [Open](pages/monitoring/jobs.md) |
| System Logs | `/logs` | System Logs — Machina Monitoring page at `/logs`. | [Open](pages/monitoring/logs.md) |
| Audit Log | `/audit` | Audit Log — Machina Monitoring page at `/audit`. | [Open](pages/monitoring/audit.md) |
| Web Sessions | `/admin/sessions` | Web Sessions — Machina Monitoring page at `/admin/sessions`. | [Open](pages/monitoring/admin-sessions.md) |

## Related

- [Customer docs home](README.md)
- [Page-by-page guides](pages/README.md)
