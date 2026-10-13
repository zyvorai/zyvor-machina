// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Shared copy for the home page, gallery and screenshot strip. Every claim maps to code in the repo.

export const REPO = 'https://github.com/zyvorai/zyvor-machina';
export const DEMO_URL = 'https://zyvor.dev/schedule?utm_source=pages&utm_medium=machina&utm_campaign=site';
export const POC_URL = 'https://zyvor.dev/poc?utm_source=pages&utm_medium=machina&utm_campaign=site';
export const INSTALL = 'git clone https://github.com/zyvorai/zyvor-machina.git machina && cd machina && ./machinactl deploy';

export type Shot = {src: string; title: string; caption: string; url: string};

export const SHOTS: Shot[] = [
  {src: '/machina-dashboard-dark.png', title: 'Mission Control', caption: 'Fleet status, live chips and a guided Get started checklist on the home page.', url: 'https://machina:5092/platform'},
  {src: '/machina-create-dark.png', title: 'Create a VM', caption: 'Install from media or clone a golden image, with a live summary rail.', url: 'https://machina:5092/create'},
  {src: '/machina-fleet-cloud-dark.png', title: 'Fleet Cloud', caption: 'Self-service instances, images, volumes, flavors and more, all native.', url: 'https://machina:5092/fleet-cloud'},
  {src: '/machina-zyra-dark.png', title: 'Zyra AI', caption: 'Remediation hub, fleet intelligence, security graph and knowledge in one place.', url: 'https://machina:5092/platform/zyra'},
  {src: '/machina-native-ebpf-dark.png', title: 'Native eBPF', caption: 'machina-bpfd datapath: flows, L7, DNS, captures and lease-gated enforcement.', url: 'https://machina:5092/platform/zyra/security/native-bpf'},
  {src: '/machina-security-dark.png', title: 'Security Center', caption: 'Zeus firewall, SOC, threat hunting, runtime enforcement and the eBPF sensor matrix.', url: 'https://machina:5092/platform/zeus/security'},
];

export type Feature = {
  eyebrow: string;
  title: string;
  body: string;
  bullets: string[];
  shot: Shot;
  doc: string;
};

export const FEATURES: Feature[] = [
  {
    eyebrow: 'Run',
    title: 'Every VM operation, in one place.',
    body: 'Create, clone, snapshot, back up and migrate KVM guests from a UI, a REST API with 1,000+ routes, a CLI or Terraform.',
    bullets: ['Cloud-init and golden images (Packer)', 'GPU and PCI passthrough', 'Networks, storage pools and nwfilters'],
    shot: SHOTS[1],
    doc: '/docs/core-concepts/virtual-machines',
  },
  {
    eyebrow: 'Reach',
    title: 'Consoles in the browser. No gateway to deploy.',
    body: 'noVNC, SPICE, serial and SSH are proxied by machina-daemon itself, behind the same RBAC and audit log as everything else.',
    bullets: ['PAM, OIDC, SAML and LDAP sign-in', 'Role-based access control', 'Audit trail for every change'],
    shot: SHOTS[5],
    doc: '/docs/core-concepts/consoles',
  },
  {
    eyebrow: 'Scale',
    title: 'A fleet, not a host.',
    body: 'A gRPC agent per hypervisor. The controller keeps desired state, fails VMs over when a host dies, balances load with DRS and live-migrates between hosts.',
    bullets: ['HA failover and host fencing', 'DRS balancing and live migration', 'Embedded SQLite, optional NATS'],
    shot: SHOTS[0],
    doc: '/docs/core-concepts/fleet-ha',
  },
  {
    eyebrow: 'Self-service',
    title: 'Fleet Cloud: a public-cloud experience on your metal.',
    body: 'Flavors, images, instances, volumes, security groups, keypairs, floating IPs, server groups, stacks, projects and load balancers, all native controller APIs.',
    bullets: ['OpenStack-style primitives, no OpenStack', 'Load balancers as host rules, no amphora VM', 'Cloud-init on every instance'],
    shot: SHOTS[2],
    doc: '/docs/core-concepts/fleet-cloud',
  },
  {
    eyebrow: 'Protect',
    title: 'Networking and security, in the kernel.',
    body: 'One eBPF service, machina-bpfd, replaces Cilium, Tetragon, kube-proxy and a separate firewall agent: service load balancing, a Kubernetes CNI, DDoS shield, VM isolation and flow visibility.',
    bullets: ['VM network policy: L3-L7, DNS, mTLS, threat feeds', 'Quarantine and just-in-time access under a lease', 'Maglev service LB and QUIC-LB at XDP'],
    shot: SHOTS[4],
    doc: '/docs/networking/ebpf-overview',
  },
  {
    eyebrow: 'Provision',
    title: 'From a BMC address to a joined host.',
    body: 'Discover or import BMCs, commission machines over PXE or BMC virtual media, write a checksum-verified OS image, and let each one join the fleet on first boot. Verified on VMs and BMC emulators; not yet on physical servers.',
    bullets: ['Redfish and IPMI power, boot device and virtual media', 'Image catalog: Ubuntu, Debian, Rocky, AlmaLinux', 'Profiles that keep N machines deployed'],
    shot: SHOTS[0],
    doc: '/docs/core-concepts/bare-metal',
  },
  {
    eyebrow: 'Operate',
    title: 'Zyra AI: an operator that asks first.',
    body: 'Autonomous diagnostics, incident correlation, rightsizing and natural-language operations, with an approval queue in front of every change.',
    bullets: ['Human approval before actions', 'Verifies every fix, one-click undo', 'Bring your own LLM; keys encrypted at rest'],
    shot: SHOTS[3],
    doc: '/docs/core-concepts/zyra-ai',
  },
];

export const WHY = [
  {problem: 'OpenStack is a six-week project and a full-time team.', answer: 'One machinactl deploy: a few Rust services, embedded SQLite (PostgreSQL when you outgrow it), a browser UI minutes later.'},
  {problem: 'VMware renewal quotes keep climbing.', answer: 'Open KVM/libvirt underneath, with HA failover, DRS and live migration on top.'},
  {problem: 'libvirt ops live in a pile of virsh scripts.', answer: 'One dashboard, a REST API, a CLI and a Terraform provider over the same model.'},
  {problem: 'Every console needs its own gateway.', answer: 'noVNC, SPICE, serial and SSH built into the daemon, with RBAC and audit.'},
  {problem: 'On-call triages the same incidents at 3 a.m.', answer: 'Zyra AI diagnoses and proposes the fix, then waits for a human approval.'},
  {problem: 'Network and storage live in other silos.', answer: 'Native eBPF does load balancing, CNI and isolation with leases that fail open; Atlas puts disks on Ceph, NFS or ZFS.'},
];

export type News = {title: string; body: string};

export const WHATS_NEW: News[] = [
  {title: 'VPC forwarding and ACLs', body: 'Peering, internet-gateway and blackhole routes and stateless network ACLs are enforced on the hosts, over the WireGuard overlay between them.'},
  {title: 'Production profile', body: 'A strict, opt-in startup check, key rollover for JWT and master keys, audit retention and a mandatory eBPF datapath.'},
  {title: 'ARM64 packages', body: 'DEB, RPM and tar bundles for amd64 and arm64, built and tested on native runners.'},
  {title: 'EC2-compatible API', body: 'Point awscli, boto3 and Terraform at the controller: 288 actions, each applied, recorded or refused by name.'},
  {title: 'PostgreSQL controller', body: 'Run the controller on PostgreSQL for large fleets or several controllers; one command migrates an SQLite site.'},
  {title: 'FluxVM backend', body: 'FluxVM VMs sit next to libvirt VMs, with eBPF networking, live migration and HA across hosts.'},
  {title: 'Boot Doctor', body: "A VM that won't boot is diagnosed and repaired offline through GuestKit, after a backup."},
  {title: 'VM quarantine', body: 'Isolate a suspect VM in the eBPF datapath under a lease that lapses on its own.'},
  {title: 'Just-in-time access', body: 'Open a VM for a set time, with two-person approval and automatic expiry.'},
  {title: 'Zyra verify and undo', body: 'Every AI action is checked afterwards and can be rolled back in one click.'},
  {title: 'VM flow history', body: 'Service map, learn and replay of VM traffic, with lateral-movement alerts.'},
  {title: 'DNS threat feeds', body: 'VM network policy blocks known-bad domains straight from threat feeds.'},
];
