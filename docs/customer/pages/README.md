# Page-by-page guides

Each guide follows: Purpose → When to use it → How to get there → What you can do → Related pages.

Every route is also listed in the [complete page index](../PAGE_INDEX.md).

## Core

| Page | What it covers |
|------|----------------|
| [Capabilities](core/capabilities.md) | Capabilities — Machina Core page at `/capabilities`. |
| [Create new guest VM](core/create.md) | Create a new libvirt VM. |
| [Node Devices](core/devices.md) | Node Devices — Machina Core page at `/devices`. |
| [Fleet](core/fleet.md) | Fleet — Machina Core page at `/fleet`. |
| [Dashboard](core/home.md) | Host dashboard — VM inventory pulse, Vessel container shortcuts when Podman/Docker is connected, and quick links. |
| [SSH — hypervisor](core/host-ssh.md) | Host SSH — Machina Core page at `/host-ssh`. |
| [Import guest VM](core/import.md) | Import VM — Machina Core page at `/import`. |
| [Systemd Services](core/services.md) | Services — Machina Core page at `/services`. |
| [Sprites](core/sprites.md) | Instant, disposable sandbox VMs — boot on libvirt/QEMU, Cloud Hypervisor, or Firecracker, TTL-reaped automatically, no persistent state. |
| [VM detail: Guest policy](core/vm-guest-policy.md) | Restrict what individual containers inside a VM may do: which networks they reach, which programs they run, which devices they open and where they write. The rules are enforced inside the guest by GuestKit's agent (`guestkitd`) with its own eBPF programs. Machina sends them through the QEMU guest agent, so no network path into the guest is needed. |
| [Virtual Machines](core/vms.md) | Virtual machine inventory for this libvirt host. |

## Fleet Cloud

| Page | What it covers |
|------|----------------|
| [Autopilot](fleet-cloud/fleet-cloud-autopilot.md) | Autopilot is capacity management for Fleet Cloud: instance groups scale ahead of the daily or weekly rush, drain from their load balancer, and sleep instead of stopping; VMs are right-sized from their own history and hosts can be consolidated. |
| [Game days](fleet-cloud/fleet-cloud-chaos.md) | Game days inject faults into your own VMs under a lease: network latency and loss, partitions, slow disks and crashes. Health probes abort the run and lift every fault when the service suffers, and each step gets a report. |
| [Create Fleet Cloud instance](fleet-cloud/fleet-cloud-create.md) | Create Instance — Machina Fleet Cloud page at `/fleet-cloud/create`. |
| [Flavors](fleet-cloud/fleet-cloud-flavors.md) | Flavors — Machina Fleet Cloud page at `/fleet-cloud/flavors`. |
| [Floating IPs](fleet-cloud/fleet-cloud-floating-ips.md) | Floating IPs — Machina Fleet Cloud page at `/fleet-cloud/floating-ips`. |
| [Heat Orchestration](fleet-cloud/fleet-cloud-heat.md) | Heat Orchestration — Machina Fleet Cloud page at `/fleet-cloud/heat`. |
| [Identity](fleet-cloud/fleet-cloud-identity.md) | Identity — Machina Fleet Cloud page at `/fleet-cloud/identity`. |
| [Images](fleet-cloud/fleet-cloud-images.md) | Images — Machina Fleet Cloud page at `/fleet-cloud/images`. |
| [Fleet Cloud Instances](fleet-cloud/fleet-cloud-instances.md) | Instances — Machina Fleet Cloud page at `/fleet-cloud/instances`. |
| [SSH keypairs](fleet-cloud/fleet-cloud-keypairs.md) | Keypairs — Machina Fleet Cloud page at `/fleet-cloud/keypairs`. |
| [Load Balancers](fleet-cloud/fleet-cloud-load-balancers.md) | Load Balancers — Machina Fleet Cloud page at `/fleet-cloud/load-balancers`. |
| [Networking](fleet-cloud/fleet-cloud-networking.md) | Networking — Machina Fleet Cloud page at `/fleet-cloud/networking`. |
| [Preemptible](fleet-cloud/fleet-cloud-preemptible.md) | Preemptible instances are VMs that can wait. When a host runs short of memory, the lowest-priority preemptible VMs are saved to disk instead of stopped, and they come back by themselves, lowest priority first, when there is room again. |
| [VM QoS](fleet-cloud/fleet-cloud-qos.md) | VM QoS — Machina Fleet Cloud page at `/fleet-cloud/qos`. |
| [Security Groups](fleet-cloud/fleet-cloud-security-groups.md) | Security Groups — Machina Fleet Cloud page at `/fleet-cloud/security-groups`. |
| [Server Groups](fleet-cloud/fleet-cloud-server-groups.md) | Server Groups — Machina Fleet Cloud page at `/fleet-cloud/server-groups`. |
| [Network topology](fleet-cloud/fleet-cloud-topology.md) | Network Topology — Machina Fleet Cloud page at `/fleet-cloud/topology`. |
| [Volume Snapshots](fleet-cloud/fleet-cloud-volume-snapshots.md) | Volume Snapshots — Machina Fleet Cloud page at `/fleet-cloud/volume-snapshots`. |
| [Volumes](fleet-cloud/fleet-cloud-volumes.md) | Volumes — Machina Fleet Cloud page at `/fleet-cloud/volumes`. |
| [VPCs and elastic compute](fleet-cloud/fleet-cloud-vpcs.md) |  |
| [Fleet Cloud](fleet-cloud/fleet-cloud.md) | Overview — Machina Fleet Cloud page at `/fleet-cloud`. |

## Infrastructure

| Page | What it covers |
|------|----------------|
| [Backups (Snapshot Backups)](infrastructure/backups.md) | Snapshot Backups — Machina Infrastructure page at `/backups`. |
| [Container Pods](infrastructure/containers-pods.md) | Podman pods via Vessel — create and manage shared-namespace container groups (not Kubernetes pods). |
| [Containers](infrastructure/containers.md) | Local Podman/Docker containers via Vessel — list, create, lifecycle, and live stats on this host. |
| [Disk Images](infrastructure/disk-images.md) | Disk Images — Machina Infrastructure page at `/disk-images`. |
| [Host Networking](infrastructure/host-networking.md) | Host Networking — Machina Infrastructure page at `/host-networking`. |
| [Networks](infrastructure/networks.md) | Networks — Machina Infrastructure page at `/networks`. |
| [Network Filters](infrastructure/nwfilters.md) | Network Filters — Machina Infrastructure page at `/nwfilters`. |
| [Secrets](infrastructure/secrets.md) | Secrets — Machina Infrastructure page at `/secrets`. |
| [Snapshots](infrastructure/snapshots.md) | Snapshots — Machina Infrastructure page at `/snapshots`. |
| [Storage pools](infrastructure/storage.md) | Storage Pools — Machina Infrastructure page at `/storage`. |

## Kubernetes

| Page | What it covers |
|------|----------------|
| [Kata + Cloud Hypervisor](kubernetes/k8s-kata.md) | Kata + Cloud Hypervisor — Machina Kubernetes page at `/k8s/kata`. |
| [Kubernetes Workloads](kubernetes/k8s-workloads.md) | K8s Workloads — Machina Kubernetes page at `/k8s/workloads`. |
| [Kubernetes](kubernetes/k8s.md) | Kubernetes — Machina Kubernetes page at `/k8s`. |

## Monitoring

| Page | What it covers |
|------|----------------|
| [Web sessions](monitoring/admin-sessions.md) | Web Sessions — Machina Monitoring page at `/admin/sessions`. |
| [Audit Log](monitoring/audit.md) | Audit Log — Machina Monitoring page at `/audit`. |
| [Live Metrics](monitoring/events.md) | Live Metrics — Machina Monitoring page at `/events`. |
| [Daemon Jobs](monitoring/jobs.md) | Daemon Jobs — Machina Monitoring page at `/jobs`. |
| [System Logs](monitoring/logs.md) | System Logs — Machina Monitoring page at `/logs`. |
| [Host overview](monitoring/node.md) | Host Overview — Machina Monitoring page at `/node`. |
| [System Check](monitoring/system-check.md) | System Check — Machina Monitoring page at `/system-check`. |

## Platform

| Page | What it covers |
|------|----------------|
| [API Docs](platform/api-docs.md) | API Docs — Platform surface. |
| [Activity Monitor](platform/platform-activity.md) | Activity Monitor — Machina Platform page at `/platform/activity`. |
| [AI Providers](platform/platform-ai-providers.md) | AI Providers — Machina Platform page at `/platform/ai-providers`. |
| [Alert Rules](platform/platform-alert-rules.md) | Threshold alert rules on VM CPU and memory: notify (via Alerts / webhooks) when a metric crosses a bound. |
| [API keys](platform/platform-api-keys.md) | API Keys — Machina Platform page at `/platform/api-keys`. |
| [Applications](platform/platform-applications.md) | Applications — Machina Platform page at `/platform/applications`. |
| [Backup Replication](platform/platform-backup-replication.md) | Backup Replication — copy completed backups to an S3-compatible store at another site, see which copies are verified, and fetch one back onto a host. Page at `/platform/backup-replication`. |
| [Backup & Restore](platform/platform-backups.md) | Fleet backups: a timeline of backup runs, backup destinations (NFS, S3/MinIO or local), recurring schedules by project or tag, and one-off backups of a single VM. |
| [Bare Metal](platform/platform-baremetal.md) | Bare Metal — Machina Platform page at `/platform/baremetal`. It takes physical servers |
| [Blueprint Studio](platform/platform-blueprints.md) | Blueprints — Machina Platform page at `/platform/blueprints`. |
| [Cloud-Init Studio](platform/platform-cloud-init.md) | Cloud-Init Studio — Machina Platform page at `/platform/cloud-init`. |
| [Content Library](platform/platform-content.md) | Images & ISOs — Machina Platform page at `/platform/content`. |
| [Advanced VM install](platform/platform-create-advanced.md) | Advanced Create — Machina Platform page at `/platform/create-advanced`. |
| [Create VM from ISO](platform/platform-create-iso.md) | Create ISO — Machina Platform page at `/platform/create-iso`. |
| [Datacenter](platform/platform-datacenter.md) | Datacenter View — Machina Platform page at `/platform/datacenter`. |
| [Desktop Pools](platform/platform-desktop-pools.md) | Desktop Pools — clone a template VM into numbered desktops, check one out per user and recycle it on release. Page at `/platform/desktop-pools`. |
| [Developer](platform/platform-developer.md) | Developer Hub — Machina Platform page at `/platform/developer`. |
| [Host Enrollment](platform/platform-enroll.md) | Join new KVM hypervisors to the control plane. You generate a one-time join token here and run the install command on the new host; its `machina-agent` then registers with the controller. |
| [Enterprise Features](platform/platform-enterprise.md) | Enterprise Features — Machina Platform page at `/platform/enterprise`. |
| [Platform events](platform/platform-events.md) | Event Log — Machina Platform page at `/platform/events`. |
| [Failure Domains](platform/platform-failure-domains.md) | Failure Domains — racks defined by host site and rack labels, and whether HA could re-home each rack if it were lost. Page at `/platform/failure-domains`. |
| [Fleet snapshot schedules](platform/platform-fleet-snapshots.md) | Fleet Snapshots — Machina Platform page at `/platform/fleet-snapshots`. |
| [GPU Command Center](platform/platform-gpu.md) | GPU Command Center — Machina Platform page at `/platform/gpu`. |
| [High Availability](platform/platform-ha.md) | Fleet high availability: which hosts are healthy, fence events, and HA restarts of VMs from failed hosts. The controller fences a failed host before it restarts that host's VMs elsewhere, so a VM never runs twice. |
| [Machine Finder](platform/platform-hosts-finder.md) | Machine Finder: opens the fleet VM list in its topology lens (/platform/vms?lens=topology) to find any VM by host, network or name. |
| [Hosts](platform/platform-hosts.md) | Every hypervisor enrolled with the controller: online/offline state, agent heartbeat, capacity and the VMs it runs. This is where you start any host-level task in the fleet. |
| [Kubernetes Clusters](platform/platform-k8s-clusters.md) | Kubernetes Clusters — create, scale, upgrade and delete k3s clusters across enrolled hosts (opt-in on the controller and each host). Page at `/platform/k8s-clusters`. |
| [Launchpad (on Mission Control)](platform/platform-launchpad.md) | Launchpad tiles live on Mission Control (`/platform`) — there is no separate `/platform/launchpad` route. |
| [Maintenance](platform/platform-maintenance.md) | Maintenance — Machina Platform page at `/platform/maintenance`. |
| [Marketplace](platform/platform-marketplace.md) | Marketplace — Machina Platform page at `/platform/marketplace`. |
| [Migration Waves](platform/platform-migration-waves.md) | Migration Waves — plan a batch of VMs for Transiva, start them a few at a time, follow each job and record cutover. Page at `/platform/migration-waves`. |
| [Migration Assistant](platform/platform-migration.md) | Migration Radar: bring VMs into Machina from OVF/OVA, VMDK or cloud images. GuestKit checks disks offline before and after conversion. |
| [Network canvas](platform/platform-network-canvas.md) | Network Canvas — Machina Platform page at `/platform/network-canvas`. |
| [Networks](platform/platform-networks.md) | The fleet network pane: libvirt networks across hosts, overlay segments with east-west policy, and IPAM pools that hand out addresses from a segment CIDR. |
| [Alerts](platform/platform-notifications.md) | Alerts — Machina Platform page at `/platform/notifications`. |
| [Observability](platform/platform-observability.md) | Observability — Machina Platform page at `/platform/observability`. |
| [Placement & HA](platform/platform-placement.md) | Disaster Recovery — Machina Platform page at `/platform/placement`. |
| [Policy rules](platform/platform-policy.md) | Policy — Machina Platform page at `/platform/policy`. |
| [Stage Manager](platform/platform-projects.md) | Stage Manager — Machina Platform page at `/platform/projects`. |
| [Recommendations](platform/platform-recommendations.md) | Recommendations — Machina Platform page at `/platform/recommendations`. |
| [Runbook catalog](platform/platform-reports.md) | Reports — Machina Platform page at `/platform/reports`. |
| [Scheduled Jobs](platform/platform-scheduled-jobs.md) | Recurring controller operations from a whitelist, for example periodic host inventory refresh. |
| [Settings](platform/platform-settings.md) | One hub for platform configuration: general and cluster preferences, identity and SSO, users and groups, API keys, AI providers, Zyra, network, policy and quotas, webhooks, reports, console, keychain and updates. |
| [Security Operations Center](platform/platform-soc.md) | Security Operations — Machina Platform page at `/platform/soc`. |
| [Storage (Atlas)](platform/platform-storage-atlas.md) | Storage (Atlas) — Machina Platform page at `/platform/storage-atlas`. |
| [Storage Tiers](platform/platform-storage-tiers.md) | Storage Tiers — Machina Platform page at `/platform/storage-tiers`. |
| [Storage (Disk Utility)](platform/platform-storage.md) | Disk Utility — Machina Platform page at `/platform/storage`. |
| [Support Assistant](platform/platform-support.md) | Support — Machina Platform page at `/platform/support`. |
| [Tasks](platform/platform-tasks.md) | The controller's orchestration queue. Every long-running operation (VM create, migrate, backup, agent upgrade, …) is a task with an operation name, status, progress and message. |
| [Marketplace (Templates)](platform/platform-templates.md) | Templates — Machina Platform page at `/platform/templates`. |
| [Topology](platform/platform-topology.md) | Topology — Machina Platform page at `/platform/topology`. |
| [Upgrade Matrix](platform/platform-upgrade.md) | Version compatibility between the controller and host agents (controller version, minimum and recommended agent) and a per-host Upgrade Agent action. |
| [Users & Groups](platform/platform-users.md) | Users & Groups — Machina Platform page at `/platform/users`. |
| [vGPU Partitions](platform/platform-vgpu.md) | vGPU Partitions — split a GPU into mediated-device partitions, give each to a VM, and see which VMs are pinned to a host by one. Page at `/platform/vgpu`. |
| [AI VM Builder](platform/platform-vm-builder.md) | VM Builder — Machina Platform page at `/platform/vm-builder`. |
| [Machine Finder](platform/platform-vms.md) | Fleet VM finder across enrolled hosts. |
| [Webhooks](platform/platform-webhooks.md) | Webhooks — Machina Platform page at `/platform/webhooks`. |
| [Mission Control](platform/platform.md) | Mission Control — multi-host platform overview. |
| [Settings](platform/settings.md) | Daemon and UI settings, including Fleet Cloud wiring. |

## Platform Security

| Page | What it covers |
|------|----------------|
| [Security Center](platform-security/platform-zeus-security.md) | Fleet security overview: the infrastructure security graph, critical findings, natural-language event search, and the eBPF sensor matrix showing `machina-bpfd` on every host as reported through its agent. |
| [Zyra Approvals](platform-security/platform-zyra-approvals.md) | Approvals — Machina Platform / Security page at `/platform/zyra/approvals`. |
| [Configure Zyra](platform-security/platform-zyra-configure.md) | Configure Zyra — Machina Platform / Security page at `/platform/zyra/configure`. |
| [Incident Commander](platform-security/platform-zyra-incidents.md) | Incident Commander — Machina Platform / Security page at `/platform/zyra/incidents`. |
| [VM Rightsizing](platform-security/platform-zyra-rightsizing.md) | Rightsizing — Machina Platform / Security page at `/platform/zyra/rightsizing`. |
| [Native eBPF](platform-security/platform-zyra-security-native-bpf.md) | Operate Machina's built-in eBPF datapath on a host. One root service, `machina-bpfd`, loads Machina's own kernel programs for flow visibility, policy enforcement, load balancing, DDoS protection, VM isolation and scheduling. This page is the console for it; there is no separate Cilium, Tetragon, Netra or PacketWolf agent to install. |
| [VM Network Policies](platform-security/platform-zyra-security-network-policies.md) | VM network policies decide which VM may talk to which, in both directions, using the CiliumNetworkPolicy schema. They compile into the eBPF edge Machina attaches to each VM's network interface; Cilium does not need to be installed. |
| [Machina Zyra OS](platform-security/platform-zyra.md) | Zyra AI assistant for Machina operations. |

---

119 guides. Regenerate: `node scripts/customer-docs/generate-guide-index.mjs`.
