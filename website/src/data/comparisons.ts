// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Competitive comparison data. Every Machina cell starts with a status
// (Yes / Partial / Planned / No) and must trace to a row in docs/claims.md
// (C56 covers this page set). Competitor cells describe the product as
// documented publicly; vendors change things, so each page carries a
// "check the vendor's current documentation" note.

export type Status = 'Yes' | 'Partial' | 'Planned' | 'No';

export interface Row {
  topic: string;
  machina: string;
  other: string;
  status: Status;
}

export interface Comparison {
  slug: string;
  name: string; // competitor display name
  title: string;
  description: string;
  lede: string;
  rows: Row[];
  machinaWhen: string[];
  otherWhen: string[];
  trademark: string;
}

export const AZURE_LOCAL: Comparison = {
  slug: 'vs-azure-local',
  name: 'Azure Local',
  title: 'Machina vs Azure Local',
  description:
    'Machina compared with Azure Local (formerly Azure Stack HCI): deployment models, storage, HA, GPUs, Kubernetes, VDI, disconnected operation and licensing, with the gaps stated.',
  lede:
    'Azure Local is the strongest answer when you want Azure as your control plane on your own hardware. Machina is for teams that want open KVM, no cloud dependency and one binary set. Here is where each fits, including what Machina does not do yet.',
  rows: [
    {topic: 'Hypervisor', status: 'Yes', machina: 'KVM/QEMU through libvirt; FluxVM engine optional', other: 'Hyper-V'},
    {topic: 'Control plane', status: 'Yes', machina: 'Runs on your hosts; no cloud account required (C47)', other: 'Azure portal and Azure Arc; connected by default, local control plane only in disconnected operations'},
    {topic: 'Cluster size and HA', status: 'Yes', machina: 'Fleet of hosts, HA failover with fencing, DRS, live migration, controller HA', other: '1 to 16 machines hyperconverged; up to 64 disaggregated; 100+ multi-rack'},
    {topic: 'Rack / room failure domains', status: 'Partial', machina: 'Hosts carry site and rack labels (C57): new VMs are steered away from a rack that already runs a member of their anti-affinity group, and a report shows whether HA could re-home a rack\'s VMs on the remaining hosts. No witness or quorum arbiter yet, and HA recovery does not prefer another rack', other: 'Rack-aware cluster, two racks, 1 ms round trip'},
    {topic: 'Storage', status: 'Partial', machina: 'Local disks, NFS and Atlas (separate Zyvor product): Ceph RBD, NFS, ZFS, Longhorn and a native distributed filesystem, with snapshots and backup. No iSCSI/Fibre Channel target yet', other: 'Storage Spaces Direct, plus Fibre Channel / iSCSI SAN and disaggregated designs'},
    {topic: 'Disconnected / sovereign operation', status: 'Partial', machina: 'No required cloud dependency (C47); offline join bundle for hosts; Zyvor Fleet (offline-first edge control plane) and Zyvor OTA (signed updates with rollback, offline) cover edge sites; an air-gapped guide lists every default internet touchpoint and its mirror setting, with an egress check (C58); no offline updater for Machina itself', other: 'Disconnected operations with a local Azure control plane, subject to an eligibility process'},
    {topic: 'GPU for VMs', status: 'Partial', machina: 'GPU Command Center: discovery, PCI/USB passthrough and fleet GPU rollup; Gravia (separate product) schedules fractional GPUs on Kubernetes; vGPU partitions through mediated devices on the daemon and agent, on hosts whose GPU driver supports mdev (C59); no controller API or placement awareness yet', other: 'Discrete device assignment and GPU partitioning'},
    {topic: 'Trusted launch / confidential VMs', status: 'Partial', machina: 'UEFI Secure Boot with keys enrolled and an emulated TPM 2.0 at VM create (C60); no confidential VMs (SEV-SNP/TDX) and no attestation', other: 'Trusted Launch GA; confidential VMs on AMD SEV-SNP in preview'},
    {topic: 'Kubernetes', status: 'Partial', machina: 'Opt-in native eBPF CNI, k3s bootstrap, KubeVirt inventory and migration. Kairon runs VMs as Kubernetes Machine resources on FluxVM (no pod per VM) and Zorvia manages KubeVirt. The controller can create, scale, upgrade and delete k3s clusters across enrolled hosts (opt-in, one server per cluster, C61)', other: 'AKS enabled by Arc, managed from the portal'},
    {topic: 'Virtual desktops', status: 'Partial', machina: 'Built-in RDP/VNC/SPICE consoles and desktop pools: a template cloned into numbered desktops, one per user, powered on at checkout and re-cloned on release (C63). No connection broker and no Windows multi-session', other: 'Windows 11 multi-session through Azure Virtual Desktop'},
    {topic: 'VM network security', status: 'Yes', machina: 'Native eBPF VM edge, security groups, microsegmentation with signed evidence', other: 'Software-defined networking features; not covered in detail by public comparisons'},
    {topic: 'Bare-metal provisioning', status: 'Yes', machina: 'BMC power, PXE/ramdisk image deploy, join (C49 to C51)', other: 'OEM deployment tooling'},
    {topic: 'VMware / Hyper-V / cloud migration', status: 'Yes', machina: 'Transiva migrates from vSphere, Hyper-V, Nutanix, Proxmox, AWS, Azure, GCP, OCI, OpenStack and Alibaba (export, convert, deploy, scheduling). h2kvm converts VMDK/VHDX/OVA/raw and GuestKit repairs the guest offline, Windows VirtIO included. Machina imports the result via Import VM. Migration waves pace a batch and track a cutover checklist (C62)', other: 'Azure Migrate for VMware, GA; Hyper-V to Azure Local in preview'},
    {topic: 'Backup and disaster recovery', status: 'Partial', machina: 'Scheduled backups with verification and targets; Atlas two-way RBD mirroring with failback and async cross-cluster replication; backups can also be copied to S3-compatible storage at another site and size-verified, with a manual restore runbook (C64). No orchestrated failover', other: 'Azure Backup / Site Recovery through Azure'},
    {topic: 'Management across hypervisors and clouds', status: 'Partial', machina: 'Transiva discovers and moves VMs across ten platforms; Veyron is the console for Kairon/FluxVM; no Arc-style attachment of foreign estates for policy and monitoring', other: 'Azure Arc governs Azure Local, vSphere, Hyper-V, Nutanix, Proxmox, AWS, GCP and Kubernetes'},
    {topic: 'Image marketplace', status: 'Yes', machina: 'Marketplace and template catalog; Zorvia ships 43 OS templates for KubeVirt', other: 'Azure Marketplace and image gallery'},
    {topic: 'Operations AI', status: 'Yes', machina: 'Zyra AI diagnostics, root cause and approvals, with a local LLM option', other: 'Azure Monitor, Defender for Cloud and Azure Policy via Arc'},
    {topic: 'Cost model', status: 'Yes', machina: 'Per-host subscription (see subscription model); no per-core cloud fee', other: 'Per physical core host fee plus Windows licensing; cloud-connected'},
  ],
  machinaWhen: [
    'You want open KVM and no dependency on a hyperscaler account.',
    'You run a lab, branch, sovereign or air-gapped site and want a single install.',
    'You want eBPF network enforcement and an EC2-compatible API.',
    'You want a flat per-host price instead of per-core plus guest OS licensing.',
  ],
  otherWhen: [
    'Your estate is Windows and Azure first, and you want one Azure portal for everything.',
    'You need Windows 11 multi-session desktops, SQL Server or Microsoft 365 Local on site.',
    'You need rack-aware clusters, SAN or disaggregated storage today.',
    'You want a vendor-certified hardware catalog and Microsoft support.',
  ],
  trademark: 'Azure, Azure Local, Azure Arc and Hyper-V are trademarks of Microsoft Corporation.',
};

export const VMWARE: Comparison = {
  slug: 'vs-vmware',
  name: 'VMware',
  title: 'Machina vs VMware',
  description: 'Machina compared with VMware vSphere / VCF for teams planning an exit: HA, DRS, live migration, networking, storage and migration tooling.',
  lede:
    'If you are leaving vSphere because of licensing, you still need HA, DRS and live migration on day one. Here is what Machina covers, what it replaces and what is missing.',
  rows: [
    {topic: 'Hypervisor', status: 'Yes', machina: 'KVM/QEMU, open source underneath', other: 'ESXi'},
    {topic: 'HA, DRS, live migration', status: 'Yes', machina: 'Built into the controller; migration oracle and adaptive actuator (unit-tested, C28 to C29)', other: 'vSphere HA, DRS, vMotion'},
    {topic: 'Fault tolerance (lockstep)', status: 'No', machina: 'Not available; use HA restart', other: 'vSphere Fault Tolerance'},
    {topic: 'Storage', status: 'Partial', machina: 'Local, NFS, Atlas-managed Ceph; no vSAN-style built-in engine', other: 'vSAN and VMFS'},
    {topic: 'Network virtualization', status: 'Partial', machina: 'eBPF security groups and microsegmentation, overlay segments, VPC peering and route tables enforced on hosts (C22, C52)', other: 'NSX'},
    {topic: 'Cloud API', status: 'Yes', machina: 'REST plus EC2-compatible API and Terraform', other: 'vCenter API; VCF Automation'},
    {topic: 'Migration in', status: 'Yes', machina: 'Transiva (vSphere and nine more providers, scheduling, cost and carbon view) with h2kvm conversion and GuestKit offline repair and migration passport; progress via web, webhooks and email; migration waves pace a batch and track a cutover checklist (C62)', other: 'N/A'},
    {topic: 'Licensing', status: 'Yes', machina: 'Per-host subscription', other: 'Per-core subscription bundles'},
    {topic: 'Ecosystem and certified hardware', status: 'No', machina: 'Smaller partner ecosystem', other: 'Very large HCL and partner network'},
  ],
  machinaWhen: [
    'You want HA and DRS on open KVM without a per-core subscription.',
    'You want an API-first, EC2-compatible private cloud.',
    'Your team is small and wants to install and upgrade it alone.',
  ],
  otherWhen: [
    'You rely on NSX, vSAN or a partner ecosystem we have not matched.',
    'You need lockstep fault tolerance or a vendor-certified hardware list.',
  ],
  trademark: 'VMware, vSphere, vSAN, vMotion and NSX are trademarks of Broadcom Inc.',
};

export const NUTANIX: Comparison = {
  slug: 'vs-nutanix',
  name: 'Nutanix',
  title: 'Machina vs Nutanix',
  description: 'Machina compared with Nutanix AHV and Prism: hyperconverged storage, HA, networking and cost.',
  lede: 'Nutanix is a mature hyperconverged platform with its own storage fabric. Machina is a KVM control plane that delegates storage. The honest difference is storage.',
  rows: [
    {topic: 'Hypervisor', status: 'Yes', machina: 'KVM/QEMU via libvirt', other: 'AHV (KVM-based) or ESXi'},
    {topic: 'Hyperconverged storage', status: 'Partial', machina: 'Delegated to Atlas (Ceph, NFS, ZFS); no built-in distributed storage', other: 'Built-in distributed storage fabric'},
    {topic: 'Management', status: 'Yes', machina: 'Web UI, REST, EC2 API, CLI, Terraform', other: 'Prism Central'},
    {topic: 'HA and balancing', status: 'Yes', machina: 'HA with fencing, DRS', other: 'AHV HA and ADS'},
    {topic: 'Network microsegmentation', status: 'Yes', machina: 'Native eBPF VM edge with evidence export', other: 'Flow microsegmentation'},
    {topic: 'Disaster recovery', status: 'Partial', machina: 'Backups plus Atlas RBD mirroring with failback and async cross-cluster replication, plus size-verified copies of backups to S3-compatible storage at another site (C64); no orchestrated failover', other: 'Async/sync replication and DR orchestration'},
    {topic: 'Kubernetes', status: 'Partial', machina: 'eBPF CNI and k3s bootstrap; Kairon for VMs as Kubernetes resources', other: 'Nutanix Kubernetes Platform'},
    {topic: 'Migration from Nutanix', status: 'Yes', machina: 'Transiva has a Nutanix provider (export, convert with h2kvm, deploy); GuestKit repairs the guest offline', other: 'N/A'},
  ],
  machinaWhen: ['You already have Ceph or NFS and want just the control plane.', 'You want eBPF enforcement and an EC2-style API.'],
  otherWhen: ['You want one vendor for compute, storage and DR with built-in replication.'],
  trademark: 'Nutanix, AHV, Prism and Flow are trademarks of Nutanix, Inc.',
};

export const PROXMOX: Comparison = {
  slug: 'vs-proxmox',
  name: 'Proxmox VE',
  title: 'Machina vs Proxmox VE',
  description: 'Machina compared with Proxmox VE: clustering, storage, HA, API, security and fleet scale.',
  lede: 'Proxmox VE is a popular open-source virtualization manager. Machina targets teams that outgrow a cluster and want a fleet, an EC2-style API and enforced network security.',
  rows: [
    {topic: 'Base', status: 'Yes', machina: 'KVM/libvirt, Rust services, React UI', other: 'Debian, KVM, LXC, ZFS, Ceph'},
    {topic: 'Built-in storage', status: 'Partial', machina: 'Atlas for Ceph/NFS/ZFS; local and NFS pools', other: 'ZFS and Ceph integrated in the installer'},
    {topic: 'Containers', status: 'Partial', machina: 'Podman/Docker (Vessel); no LXC', other: 'LXC built in'},
    {topic: 'Multi-host scale', status: 'Yes', machina: 'Controller fleet with DB (SQLite or PostgreSQL)', other: 'Corosync cluster; Datacenter Manager for multiple clusters'},
    {topic: 'API', status: 'Yes', machina: 'REST, EC2-compatible, Terraform', other: 'REST API'},
    {topic: 'Network security', status: 'Yes', machina: 'eBPF security groups and microsegmentation', other: 'Firewall and SDN features'},
    {topic: 'Migration from Proxmox', status: 'Yes', machina: 'Transiva has a Proxmox provider; h2kvm converts disks and GuestKit fixes the guest', other: 'N/A'},
    {topic: 'Cost', status: 'Yes', machina: 'Per-host subscription', other: 'Free; optional support subscriptions'},
  ],
  machinaWhen: ['You need a fleet view, audit trail and an EC2 API.', 'You want AI-assisted operations with approvals.'],
  otherWhen: ['You want the lowest cost and a large community.', 'You rely on LXC or integrated ZFS/Ceph.'],
  trademark: 'Proxmox is a trademark of Proxmox Server Solutions GmbH.',
};

export const ALL = [AZURE_LOCAL, VMWARE, NUTANIX, PROXMOX];
