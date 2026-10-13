# Machina documentation

Machina is a private cloud for KVM: a single-host daemon, a multi-host controller with an agent per host, a native
eBPF datapath (`machina-bpfd`) and a React web UI. The public site with the same material in tutorial form is
[zyvorai.github.io/zyvor-machina](https://zyvorai.github.io/zyvor-machina/).

## Start here

| You are… | Read |
|----------|------|
| Evaluating Machina | [../README.md](../README.md), then the [website](https://zyvorai.github.io/zyvor-machina/docs/intro) |
| Installing for the first time | [QUICKSTART.md](QUICKSTART.md) — controller, node and first VM in one page; automation in [../deploy/](../deploy/README.md) |
| Installing or running it | [handbook/README.md](handbook/README.md) — product guide, admin configuration, runbook, FAQ, troubleshooting |
| Using the web UI | [customer/README.md](customer/README.md) — page-by-page manual and PDFs; [machina-customer-feature-guide.md](machina-customer-feature-guide.md) |
| Preparing a customer pilot | [CUSTOMER_SITE_READINESS.md](CUSTOMER_SITE_READINESS.md), [PRODUCTION_READINESS.md](PRODUCTION_READINESS.md) |
| A new engineer | [ENGINEERING_ONBOARDING.md](ENGINEERING_ONBOARDING.md), then [../CLAUDE.md](../CLAUDE.md) for architecture and conventions |

## Learn and decide

| You want to… | Read |
|----------|------|
| Try it hands-on | [tutorials/](tutorials/README.md): first cloud, EC2 in ten minutes, scale and survive |
| Look up one Fleet Cloud feature | [guides/](guides/README.md) |
| Get tasks done quickly | [users/cookbook.md](users/cookbook.md) |
| Decide whether to adopt it | [buyers/why-machina.md](buyers/why-machina.md), [alternatives](buyers/machina-vs-alternatives.md), [security](buyers/security-and-compliance.md), [30-day evaluation](buyers/evaluation-guide.md) |
| Check what is proven | [claims.md](claims.md) |
| Replace OpenStack | [migration/from-openstack.md](migration/from-openstack.md): service map, concept map, gaps |
| Choose SQLite or PostgreSQL | [guides/database.md](guides/database.md) |
| Use `aws`, boto3 or Terraform | [cloud-ec2-api.md](cloud-ec2-api.md), [cloud-ec2-clients.md](cloud-ec2-clients.md), [cloud-ec2-semantics.md](cloud-ec2-semantics.md) |

## Operate

| Topic | Document |
|-------|----------|
| Operator runbook (install, upgrade, backup, fleet playbooks, eBPF procedures) | [handbook/runbook.md](handbook/runbook.md) |
| Configuration and environment variables | [handbook/admin-configuration.md](handbook/admin-configuration.md) |
| Ports (single table) | [handbook/README.md#ports](handbook/README.md#ports) |
| Troubleshooting | [handbook/troubleshooting.md](handbook/troubleshooting.md) |
| Controller platform: setup, enrollment, API, E2E | [platform.md](platform.md) |
| Controller HA, fencing, DRS, multiple controllers | [controller-ha.md](controller-ha.md) |
| Daemon peer fleet (no controller) | [daemon-peer-fleet.md](daemon-peer-fleet.md) |
| Migration planning: predictive oracle, adaptive engine | [migration/predictive-oracle.md](migration/predictive-oracle.md), [adaptive-engine](migration/adaptive-engine.md) |
| VM performance autopilot | [ai/performance-autopilot.md](ai/performance-autopilot.md) |
| Black Box recorder, root cause, noisy neighbours | [ebpf/README.md](ebpf/README.md) |
| Live migration actuator | [migration/live-actuator.md](migration/live-actuator.md) |
| Certification matrix | [platform-cert-matrix.md](platform-cert-matrix.md) |
| Compliance hardening | [compliance-hardening.md](compliance-hardening.md) |
| Production profile, key rollover, audit retention, qualification gates | [../website/docs/operations/production-hardening.md](../website/docs/operations/production-hardening.md) |
| Packaging and remote builds | [PACKAGE_BINARY_REMOTE.md](PACKAGE_BINARY_REMOTE.md), [CLIENT_BUNDLE_POLICY.md](CLIENT_BUNDLE_POLICY.md), [macos-build.md](macos-build.md) |
| Observability | [guides/observability.md](guides/observability.md) |

## Networking and security (native eBPF)

| Topic | Document |
|-------|----------|
| Overview: architecture, kernel requirements, safety model, API map, tests | [ebpf/README.md](ebpf/README.md) |
| Datapath: load balancing, shield, node isolation, TCP health, TLS | [ebpf/datapath.md](ebpf/datapath.md) |
| Enforcement: policies, VM edge, QEMU sandbox, VMM guard, direct redirect | [ebpf/enforcement.md](ebpf/enforcement.md) |
| Observability: flows, L7, network-change audit, VM runtime | [ebpf/observability.md](ebpf/observability.md) |
| Fast path: QUIC-LB, AF_XDP, sched_ext | [ebpf/fastpath.md](ebpf/fastpath.md) |
| Kubernetes CNI (`machina-cni`) | [ebpf/cni.md](ebpf/cni.md) |
| Guest per-container policy (GuestKit) | [ebpf/guest-policy.md](ebpf/guest-policy.md) |
| SOC integrations | [soc-integrations.md](soc-integrations.md) |

## Authentication and integrations

| Topic | Document |
|-------|----------|
| LDAP sign-in | [ldap-auth.md](ldap-auth.md) |
| OIDC and run-as-user | [oidc-run-as-user.md](oidc-run-as-user.md), [oidc-effective-linux-user.md](oidc-effective-linux-user.md) |
| Active Directory | [guides/ad-integration-zyvorai.md](guides/ad-integration-zyvorai.md) |
| Atlas storage (Ceph / NFS / ZFS volumes) | [atlas-storage.md](atlas-storage.md) |
| KubeVirt migration | [kubevirt-migration.md](kubevirt-migration.md) |
| FluxVM backend (QEMU / Cloud Hypervisor / Firecracker / flux-vm VMs, fleet migration and HA) | [fluxvm.md](fluxvm.md) |
| Consoles: console hub, built-in RDP, cinema mode | [consolehub-architecture.md](consolehub-architecture.md), [builtin-rdp.md](builtin-rdp.md), [machina-cinema-mode.md](machina-cinema-mode.md) |
| VM access and lifecycle guides | [guides/vm-daily-access.md](guides/vm-daily-access.md), [guides/vm-lifecycle-ssh.md](guides/vm-lifecycle-ssh.md), [guides/integrations.md](guides/integrations.md) |

## Web UI and design

Machina's look follows the Netra design: two themes (light and dark), apple.com blue `#0071e3`, flat 18px cards,
neutral hairlines. The shell is a top bar (`GlobalBar`) with product flyouts and a chapter bar; there is no sidebar.

| Topic | Document |
|-------|----------|
| UX contract (surface tiers, laws, theme map) | [design/APPLE-UX-CONTRACT.md](design/APPLE-UX-CONTRACT.md) |
| Light and dark tokens | [design/DAYLIGHT-CONTRACT.md](design/DAYLIGHT-CONTRACT.md) |
| UX author guide | [ux.md](ux.md) |
| VM detail UX, platform feature Q&A | [guides/platform-vm-detail-ux.md](guides/platform-vm-detail-ux.md), [guides/platform-feature-qa.md](guides/platform-feature-qa.md) |
| UX end-to-end and API-to-UI coverage | [ux-e2e-coverage.md](ux-e2e-coverage.md), [api-ux-coverage.md](api-ux-coverage.md) |

The design contracts are enforced with `node web/scripts/ux-audit.mjs`.

## API reference

- OpenAPI: [openapi-daemon.json](openapi-daemon.json), [openapi-controller.json](openapi-controller.json)
  (regenerate with `cd web && npm run generate-openapi`; CI checks for drift).
- Coverage manifests: [api-ux-route-manifest.json](api-ux-route-manifest.json),
  [ux-wiring-live-manifest.json](ux-wiring-live-manifest.json).
- Live e2e runs write `docs/e2e-last-run.json` and `docs/ux-wiring-live-report.json`; those are generated and not
  committed.

## Product, licensing and assets

| Topic | Document |
|-------|----------|
| User journeys and acceptance criteria | [USER_STORIES.md](USER_STORIES.md) |
| Subscription model (Zyvor Production License) | [SUBSCRIPTION-MODEL.md](SUBSCRIPTION-MODEL.md), [legal/README.md](legal/README.md) |
| Social and launch cards | [social/README.md](social/README.md) |
| Client presentations | [client-presentations/](client-presentations/) |
| Changelog | [../CHANGELOG.md](../CHANGELOG.md) |
| Archived snapshots (historical, not maintained) | [archive/](archive/) |

## Ecosystem

Part of the [Zyvor stack](https://zyvor.dev):

| Product | Role |
|---------|------|
| **machina** | KVM private cloud: VMs, consoles, native eBPF networking and enforcement, fleet HA, Fleet Cloud |
| **atlas** | Ceph / NFS / ZFS storage control plane (`ATLAS_*` integration) |
| **guestkit** | Guest agent, per-container eBPF policy inside guests, offline disk inspection |
| **zeus-os (v9s)** | KubeVirt-based cloud that can run on top of Machina hosts |
| **hyper2kvm** | VM migration into KVM |

Machina's native eBPF stack (`machina-bpfd`, `machina-cni`) replaces Cilium, Tetragon, Netra and PacketWolf; it no
longer integrates with any of them.

- [VPC foundations and elastic compute](cloud-vpc-elastic-compute.md) — project-owned isolated subnets, IPAM, launch templates and instance groups.
- [Cloud fabric](cloud-fabric.md) — VPC peering, internet-gateway and blackhole forwarding and network ACLs enforced on the hosts, over the WireGuard overlay.
- [EC2-compatible API](cloud-ec2-api.md) — the EC2, Auto Scaling, ELBv2 and alarm query endpoints: a per-action table (real, recorded, plan-only, refused), what is unit-tested and what has been run. [Using stock clients](cloud-ec2-clients.md) covers the aws cli, boto3 and Terraform.
