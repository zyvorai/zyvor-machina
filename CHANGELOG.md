# Changelog

## v0.3.0 — 2026-10-17

Everything merged since v0.2.1 (#140 to #171). The detailed entries are the dated sections below.

- **Cloud fabric.** VPC peering, internet-gateway and blackhole routes and EC2 network ACLs are programmed on the
  hosts, including same-host peerings, with immediate convergence (#166, #168).
- **Production profile and security lifecycle.** `MACHINA_SECURITY_PROFILE=production` preflight, API-key master key
  file, JWT secret rollover, audit retention, `machina-bpfd --require-programs` (#166).
- **ARM64.** Native arm64 CI and a dual-architecture release workflow with per-architecture SBOM and checksums (#166).
- **Transiva gateway.** `/api/v1/transiva/*` status, providers, preview, submit and batch (#166).
- **Bare-metal provisioning.** BMC discovery, Redfish and IPMI control, virtual media, SOL console, verified OS
  images, image deploy, profiles and zero-touch join (#145 to #165).
- **Local-first and air-gap.** Azure touchpoints removed, keyless local LLM, Atlas reachability and headroom checks,
  mirror overrides for k3s, Kata and Firecracker (#140 to #143).
- **Fixes and docs.** Fencing IPMI passwords encrypted at rest (#150); leaving-Nutanix guide (#170, #171); website,
  README and claims updates.

## 2026-10-13 (later) — Same-host peering, immediate convergence, cross-host verification

- **Fixed:** two peered VPCs on the **same host** never forwarded (the rules only matched `machina-wg`) while the API
  reported `forwarding_active: true`. The controller now emits `local_routes` and the agent forwards between the
  bridges, one-sided like the other routes.
- **Convergence.** Native route delete, EC2 network-ACL entry create/replace/delete and association change, and
  internet-gateway attach/detach now converge the hosts when the call returns instead of waiting up to 30 s.
  `CreateNetworkAclEntry` reports the real `forwardingActive`.
- **Verified (same host).** After the fix, two peered VPCs on one host forward bridge to bridge; ACL, blackhole and
  route-delete changes converge in about 2 s (previously 6-27 s).
- **Verified.** Cross-host peering over WireGuard, ACL deny/allow and blackhole routes passed with real packets on
  two hosts; claim C52 is now `verified (cross-host, netns)`. Not run: gateway egress, IPv6, ACLs against guest VMs.

## 2026-10-13 — Cloud fabric, production profile, ARM64 packages, Transiva gateway

See [docs/cloud-fabric.md](docs/cloud-fabric.md) and
[Production hardening](website/docs/operations/production-hardening.md). Claims C52 to C55 (#166).

- **VPC forwarding.** Accepted peerings, internet-gateway routes and blackholes are installed on the hosts and EC2
  network ACLs are rendered into an early nftables chain (`machina_cloud_acl`, priority -250) with an implicit deny.
  Convergence is immediate on a route or peering change and every 30 s for drift. Peering reuses the `machina-wg`
  overlay: `AllowedIPs` (source ownership) are separate from the installed routes, IPv4 and IPv6 are programmed
  correctly, and a peering turns the overlay on without touching the VM-network-policy setting.
- **Honest status.** Peering accept and route calls return `forwarding_active` / `forwardingActive` from the real apply
  instead of a permanent `false`; instance and interface targets stay stored plans. The EC2 action table is
  regenerated (peering, internet gateway and ACL actions are no longer `plan`).
- **Security lifecycle.** `MACHINA_SECURITY_PROFILE=production` preflight, `MACHINA_API_KEY_MASTER_KEY_FILE`,
  `MACHINA_JWT_PREVIOUS_SECRETS` rollover, `MACHINA_AUDIT_RETENTION_DAYS` (daily, leader only), and
  `machina-bpfd --require-programs` / `MACHINA_BPF_REQUIRED`.
- **Transiva gateway.** `/api/v1/transiva/{status,providers,migrations/preview,submit,batch}`.
- **ARM64.** Native arm64 CI, architecture-neutral nfpm packages, `build-release.sh --arch`, dual-architecture release
  workflow with per-architecture SBOM and checksums.
- **Qualification gates.** `scripts/qualification/*` and `scripts/compliance/generate-evidence.py`; not run in CI.
- **Verification.** Compiled, unit-tested (agent 72, bpf 152, controller 531) and installed on a lab host; a
  single-host peering, route and ACL-hook run passed. Cross-host peering, gateway egress, ACL filtering of real
  traffic, arm64 and the qualification scripts have not run.

## 2026-10-12 — Bare-metal provisioning: BMCs to joined hosts

See [docs/baremetal-provisioning.md](docs/baremetal-provisioning.md). Claims C49 to C51.

- **Pipeline.** Discover or import BMCs, commission them (PXE or Redfish virtual media), pool by zone, allocate,
  deploy a checksum-verified OS image (or an Ubuntu/RHEL-family installer), join over the pinned TLS listener with
  site/rack/tags and named networks and pools. Profiles, one-step `provision`, auto top-up, opt-in auto-enrol
  (#145, #146, #154, #152).
- **BMC control.** Redfish and IPMI power are idempotent and read back the real state; boot-device override;
  virtual media; a Redfish read model (health, temperatures, power, firmware, log); an IPMI serial-over-LAN
  console; `host:port`; a `cycle` fallback. BMC and fencing passwords are encrypted at rest (#148, #150, #151,
  #153, #154, #155).
- **Images.** Catalog (Ubuntu 26.04/24.04/22.04, Debian 12/13, Rocky 9, AlmaLinux 9) with upstream-checksum
  verification, retries and `Range` serving (#146, #147).
- **Later in the day.** `PUT /api/v1/baremetal/servers/{id}/bmc` and `machinactl metal bmc-set`; UEFI and virtual-media
  deploy, offline bundle `--join` and the failing-join path verified on the lab host; Bare Metal page component
  tests (jsdom suites need Node 22) and an image-delete confirmation, run in headless Chrome (#159 to #164).
- **Docs.** Website pages for concepts, getting started, API/CLI/configuration and troubleshooting.
- **Operators.** `machinactl bootstrap`, `host add --file --parallel`, `metal …`, bundle `install.sh --join` (#146).
- **Verification.** The flow has run end to end against VMs and BMC emulators (sushy-tools, VirtualBMC) on a lab
  host; nothing has run on physical servers or BMCs. Migrations 074 to 083.
- **Fixed on the way.** `main` did not compile at #145 (a private module); `machinactl bootstrap` rejected every
  URL (a bracket expression); IPMI addresses with a port; a plain-http join left the agent on loopback.

## 2026-10-08 — EC2 compatibility: one endpoint, four services, one reference

See [docs/cloud-ec2-api.md](docs/cloud-ec2-api.md) and [docs/cloud-ec2-clients.md](docs/cloud-ec2-clients.md).

- **Services.** `POST /ec2`, `/autoscaling`, `/elbv2` and `/monitoring` reach one SigV4 handler; the service in the credential
  scope picks the action table and response shape (E4, #116). Auto Scaling runs on the instance groups (E5, #120); ELBv2 on the native
  layer-4 balancer (E6, #117).
- **Behaviour.** `DryRun`, `ClientToken`, one filter engine and `MaxResults`/`NextToken` on the `ec2` service (E1, #115); VPC
  networking objects, mostly stored plans (E2, #122); instance attributes, launch-template versions, placement groups, spot and
  instant fleets (E3, #121). Migrations 067 to 072.
- **Docs.** The API reference now has a generated per-action status table (`scripts/ec2/action_table.py`), and client scripts and a
  Terraform config are in `scripts/ec2/` (#119). Everything after the original endpoint is unit-tested only: no boto3, aws cli or
  Terraform run has exercised it (claims C39 to C44).

## 2026-10-08 — FluxVM: NICs on namespace VMs, backups on every engine

See [docs/fluxvm.md](docs/fluxvm.md).

- **NICs.** Extra NICs work on QEMU VMs whose primary NIC is in its own network namespace; FluxVM hands QEMU the
  host-bridge taps as open file descriptors, at hot-add and on every restart. The **Manage** tab no longer hides NIC
  controls for these VMs. Removing a VM's last extra NIC now blocks migration until it restarts (the running VM keeps
  the PCIe layout it booted with); the controller pre-check says so.
- **Backups.** Stopped VMs on every engine (Cloud Hypervisor, Firecracker, flux-vm) can be backed up and restored,
  on FluxVM's default storage or a shared disk. Backing up a running VM still needs QEMU with default storage; the UI
  disables **Back up now** for running VMs on the other engines. A restore keeps the disk's format (raw stays raw).
- **Regression.** `ops-fluxvm.js` adds a namespace NIC hot-add + restart + removal and a Firecracker backup/restore
  (live backup refused, stopped backup, restore, boot). 35/35 on two hosts (212.8.248.187 and 175.110.122.71).
- **FluxVM upgrade note.** FluxVM `main` now defaults to its native eBPF VM edge with `required = true`: on a host whose
  `/etc/fluxvm.toml` has no `[sandbox.dataplane]` mode, creating or starting a networked FluxVM VM from Machina fails
  if the BPF objects can't attach (it used to fall back to nftables). Install the BPF objects first, or set
  `mode = "legacy"`. See [docs/fluxvm.md](docs/fluxvm.md#upgrading-fluxvm).
- **FluxVM install ISOs.** `fluxvm_isos` on create (UI: **Install ISOs**) attaches up to four ISOs as CD-ROMs on a
  QEMU VM (`auto` picks QEMU). **Manage → Install media** lists them with **Eject**
  (`POST …/cdrom/eject/{drive}?backend=fluxvm`, live; the empty drive stays). Migration, the **Migrate** button and the
  controller pre-check refuse a VM with an ISO still in a drive; re-inserting is refused (FluxVM can't refill a drive).
- **FluxVM HA re-create fixes.** Re-create had stopped accepting hot-plugged VMs (it matched the old wording of the
  migration blocker); it now checks only engine and storage. Ejected drives are left out of the re-created VM, which
  FluxVM would otherwise refuse.

## 2026-10-07 — FluxVM: full VM management and fleet HA

See [docs/fluxvm.md](docs/fluxvm.md).

- **Daemon.** FluxVM VMs get snapshots, backups (QEMU), add-only vCPU/memory hot-add (QEMU, Cloud Hypervisor),
  extra NICs on host-bridge QEMU VMs, live migration (to the same host or another fluxvm-api; QEMU on a shared disk)
  and a guest-agent console (`/ws/v1/fluxvm-console/{name}`). Create takes a kernel/initrd/args, the agent switch and
  a shared disk. `wss://` fluxvm-api works, with `[fluxvm] insecure_tls` for self-signed certs.
- **Web.** VM detail → **Manage** and **Agent console** tabs on FluxVM VMs, showing what each engine supports.
- **Fleet.** `machina-agent` reports FluxVM VMs (`MACHINA_FLUXVM_URL`); the controller keeps them in inventory,
  powers and deletes them, migrates them between hosts (`vm.migrate`, DRS) and re-creates them on another host for HA
  (`ha.recover`, `POST /api/v1/vms/{id}/fluxvm/recover`). Migration `068`.
- **Tests.** `make regression-fluxvm`: 24/24 on the lab host. Migration and HA were run host-to-itself only.

## 2026-10-06 — Fleet Cloud: EC2 semantics and API

See [docs/cloud-ec2-semantics.md](docs/cloud-ec2-semantics.md) and [docs/cloud-ec2-api.md](docs/cloud-ec2-api.md).

- **Identity.** Tags and EC2-style ids on instances, volumes, security groups, key pairs, images, interfaces; tag filters;
  project-scoped API keys (`projects` on `POST /api/v1/api-keys`).
- **Compute.** Instance types that stick and `change-type`; free-form user data; key pair by name; run-instances with
  `count`/`min_count`; terminated instances stay visible for an hour; an instance metadata service at 169.254.169.254.
- **Storage.** Delete-on-termination and per-volume I/O limits; volumes from snapshots; image visibility and per-project
  sharing, enforced at launch; creating a volume now returns its path so local volumes attach.
- **Network.** Security groups enforced by the VM edge (audit by default, dry-run preview, honest status); reserved interface
  addresses pinned as DHCP host entries; Elastic IPs; a NAT gateway; subnet delete with libvirt teardown; load balancer health
  checks.
- **Scaling.** Alarms with a step-scaling action on instance groups; metric statistics; delete instance groups and launch
  templates.
- **EC2 endpoint.** `POST /ec2` with SigV4: describe, volume, security group, image, VPC, key pair, reboot, type change, run,
  terminate and tag actions. Access keys under `/api/v1/ec2/access-keys`.
- **Also.** NoCloud `meta-data` is now JSON (cirros and other minimal readers accept it). Migrations `045`–`059`.
- **Tests.** Live checks on the lab host with throwaway guests; see the verified list at the top of the semantics doc.

## 2026-10-05 — Fleet Cloud: preemptible instances

See [docs/fleet-cloud-features.md](docs/fleet-cloud-features.md#preemptible-instances).

- **Preemption.** VMs flagged preemptible (priority 0–100) are saved to
  disk, lowest priority first, when their host drops below its free-memory
  reserve, and restored, highest priority first, once the host has the
  reserve plus 5% to spare. A regular VM that fits nowhere makes room the
  same way. Preempted VMs don't wake on traffic.
- **API.** `/api/v1/preemption`, `/api/v1/preemption/settings`,
  `/api/v1/vms/{id}/preemptible`; `preemptible` and `preempt_priority` on
  VM create. Migration `054_preemptible.sql`.
- **UI.** Fleet Cloud → Preemptible; a Preemptible box on the create form.
- **Tests.** `scripts/fleet/preempt-realvm.sh` on real VMs.

## 2026-10-05 — Fleet Cloud: game days

See [docs/fleet-cloud-features.md](docs/fleet-cloud-features.md#game-days).

- **Faults.** bpfd applies latency and loss (`tc netem` on the VM tap, the
  previous qdisc put back) and partitions (bridge table `machina_chaos`)
  under a lease that ends on its own and survives a bpfd restart. New bpfd
  requests `vm_chaos_start`, `vm_chaos_stop`, `vm_chaos_status`.
- **Experiments.** `/api/v1/chaos/*`: steps for latency, loss, partition,
  disk throttle, kill (recovery timed) and simulated host failure, with
  tcp/http/VM-running probes and an abort rule. Runs need the name typed,
  skip `chaos=protected` VMs, and end with a per-phase report.
- **UI.** Fleet Cloud → Game days.
- **Upgrade.** Hosts need the new `machina-agent` as well as `machina-bpfd`:
  the agent relays the chaos requests to bpfd and runs the disk limit.
- **Tests.** `scripts/fleet/chaos-realvm.sh` on real VMs; chaos checks in
  `scripts/bpf/vm-edge-smoke.sh`.

## 2026-10-05 — Fleet Cloud: autopilot capacity

See [docs/fleet-cloud-features.md](docs/fleet-cloud-features.md#autopilot-capacity).

- **Instance groups.** Scaling policies take `predictive`, `scale_in`
  (`stop` / `sleep`), `load_balancer` and `drain_secs`. Scale-in takes the
  member out of the load balancer, waits for the drain, then sleeps or
  stops it. Scale-out restores a sleeping member and puts it back.
- **History.** Disk IOPS, network bytes and per-group CPU are sampled too,
  and everything is rolled up hourly (average, peak) for 35 days.
- **Forecast.** Weekly or daily seasonal forecast per group;
  `GET /api/v1/cloud/instance-groups/{id}/forecast`. Predictive groups are
  raised before the rush and never lowered by it.
- **Rightsizing.** `GET /api/v1/rightsizing` sizes running VMs from the
  95th percentile of 14 days of hourly peaks. `POST /api/v1/rightsizing/propose`
  files a `vm.resize` action, verified afterwards and undoable.
- **Consolidation.** `GET /api/v1/drs/consolidation` plans live migrations
  that empty under-used hosts; `POST .../propose` files a
  `drs.consolidate` action.
- **UI.** Autoscale panel on each instance group (with members and a
  demand chart) and a new Fleet Cloud → Autopilot page.
- **Fix.** `CloudCidr::last()` no longer overflows on a /32.
- **Tests.** `scripts/fleet/autopilot-realvm.sh` on real VMs.

## 2026-10-05 — Fleet Cloud: stacks you describe

See [docs/fleet-cloud-features.md](docs/fleet-cloud-features.md#stacks-you-describe).

- **Stack v2 templates.** Instance groups (count, flavor, image, network,
  labels, anti-affinity, HA, auto-sleep, restore points, backups) and
  group-to-group policies compiled into VM network policies.
- **Drafting.** `POST /api/v1/stacks/draft` turns a description into a
  template with the configured LLM (validated, one repair round), falling
  back to a rule-based parser.
- **Dry run.** `POST /api/v1/stacks/plan`: batch quota, placement
  simulation, monthly cost, policy replay against observed flows, and the
  diff for an update.
- **Approval.** `POST /api/v1/stacks/propose` files a `stack.deploy`
  action; undo deletes the new stack or restores the previous template.
- **Drift.** A 60-second reconcile loop records drift and, with auto-heal,
  recreates missing VMs and restores labels, policies and backup
  schedules. `PUT /api/v1/stacks/{id}` updates with rollback.
- **UI.** Compose stack on Fleet Cloud → Stacks; Drift and Template tabs on
  stack detail.
- **Tests.** `scripts/fleet/stack-realvm.sh` on real VMs.

## 2026-10-05 — Fleet Cloud: time travel

See [docs/fleet-cloud-features.md](docs/fleet-cloud-features.md#time-travel).

- **Restore points.** `POST /api/v1/vms/{id}/restore-points` takes an
  external disk-only snapshot of every disk, quiesced through the guest
  agent, while the VM keeps running. A schedule (`PUT
  .../restore-points/policy`) takes one every N minutes and keeps the newest
  K; older ones are merged with a live block commit.
- **Fork.** `POST /api/v1/vms/{id}/fork` makes a new VM on thin overlays
  over the source's layers, now or from any point, with new MACs and a new
  cloud-init identity. `memory: true` also copies the RAM and attaches the
  fork to the isolated `machina-fork` network. `.../fork/detach` copies the
  layers so the fork stands alone.
- **Rewind.** `POST .../restore-points/{point}/rewind` drops everything
  written after the point and restarts the VM; refused while a fork depends
  on a later point.
- **UI and CLI.** A Time travel card on instance detail (slider, rewind,
  fork, schedule). `machinactl vm restore-points|restore-point|rewind|fork|fork-detach`.
- **Tests.** `scripts/fleet/vm-timetravel-realvm.sh` on a real VM.

## 2026-10-05 — Fleet Cloud: scale to zero

See [docs/fleet-cloud-features.md](docs/fleet-cloud-features.md#scale-to-zero).

- **Sleep and wake.** `POST /api/v1/vms/{id}/sleep` managed-saves a running
  VM; its RAM and vCPUs go back to the host. The VM's addresses go into the
  host bpfd's wake set: the nftables tables `inet machina_wake` (forward and
  output) and `bridge machina_wake` (ARP), which only count packets. When a
  counter moves, bpfd publishes `vm_wake`, the agent restores the VM, and
  the controller returns it to `running`. The agent only restores a domain
  that is off and has a managed save.
- **Auto-sleep.** A VM that stays below 3 % CPU and 2 KiB/s of NIC traffic
  for its policy's minutes is put to sleep. Policy is per VM (`null`
  inherits, `0` never) or per project (`/api/v1/sleep/policies/{project}`).
  Inventory now reports cumulative NIC bytes per VM (`VmSummary.net_bytes`).
- **UI and CLI.** Instances have a Sleeping filter and Sleep/Wake buttons;
  instance detail has a Scale to zero card. `machinactl vm sleep|wake|sleep-policy|sleeping`.
- **Tests.** `vm-edge-smoke.sh` covers the wake tables (a packet from the
  host, a routed packet, a bridge ARP). `vm-netpol-realvm.sh` sleeps a real
  VM and wakes it with HTTP requests from the host and from another VM.

## 2026-10-04 — Egress IPs added by Machina, agentless VM addresses, cross-host overlay

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#cross-host-overlay-wireguard).

- **Egress IPs are added for you.** When a project egress IP is not on the
  host, `machina-bpfd` adds it (`/32` or `/128`) to the default route's
  interface, announces it with a gratuitous ARP or unsolicited neighbour
  advertisement, and removes it with the rule. Only addresses it added are
  removed. `MACHINA_NETPOL_EGRESS_MANAGE=0` turns it off;
  `MACHINA_NETPOL_EGRESS_INTERFACE` picks the interface.
- **VM addresses without the guest agent.** Inventory falls back to the
  host's ARP and NDP tables, matched by the VM's NIC MACs, so static IPv6
  addresses and second NICs are found without `qemu-guest-agent`. bpfd also
  learns source addresses from VM tap traffic (16 per VM, 15-minute expiry);
  the controller adds them to identities unless a host or another VM on the
  same host owns them.
- **Encrypted cross-host overlay.** `netpol overlay enable --fleet` connects
  hosts with WireGuard (`machina-wg`, UDP 51871). Each VM gets a fleet
  address; traffic between hosts keeps the sending VM's identity, so project
  isolation works across NATed hosts. Peers may only send from their own
  prefixes, new connections are only accepted to mapped VMs, and unpeered
  fleet traffic is refused instead of leaving through the uplink. Private
  keys never leave the host and go to the kernel over netlink, so Ubuntu's
  AppArmor profile for `wg` (keys only under `/etc/wireguard`) does not get
  in the way. `machinactl deps` installs `wireguard-tools`.
- **Tests.** `vm-edge-smoke.sh` covers managed egress addresses (ARP/NA
  seen by a netns), learned addresses and the overlay against a netns
  WireGuard peer; `vm-netpol-realvm.sh` covers learned addresses and
  enabling and disabling the overlay with real VMs.

## 2026-10-04 — IPv6 egress IPs on real VMs, second-admin approval, agent restarts

- **Guest addresses from the guest agent.** VM addresses always include
  what `qemu-guest-agent` reports (every NIC, IPv6) when its channel is
  connected, not only when DHCP leases had no IPv4. Disconnected channels
  are not queried, so inventory never waits on a missing agent.
- **Approve or withdraw project changes from the CLI.**
  `netpol project approve ID` (another admin) and `netpol project reject ID`.
  Project-change approvals now appear in evidence exports.
- **Agent restarts.** `machina-agent.service` reserves ports 50051–50052
  from the ephemeral range, so a local keep-alive connection can no longer
  take the port and fail the restart with "address already in use".
- **Tests.** `vm-netpol-realvm.sh` checks IPv6 egress IPs end to end
  (guest-agent ULA → `ip6 machina_egress` → seen as the egress IP) and a
  project change proposed, refused for self-approval, rejected, and
  approved by a temporary second admin. The mocked network-policy
  Playwright spec waits up to 15 s, so it no longer flakes under a full parallel run.

## 2026-10-04 — Fleet project networking: gaps closed

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#project-isolation-fleet-cloud).

- **All host and VM addresses.** `machina-bpfd` reports every global
  address of its host (`node_addrs`); the controller uses them for `host`
  and `remote-node` instead of the management address alone. VMs carry
  every guest address (all NICs, IPv4 and IPv6; controller column
  `vms.guest_ips`, migration 036), not only the first IPv4.
- **Cross-host NAT warning.** Projects spread over hosts whose VM subnets
  are NATed per host are flagged in the API, CLI, UI and evidence.
- **Egress IP gaps.** VMs running on a host without their project's egress
  IP are reported (`netpol.egress_gap` events, `egress_gaps`, evidence
  warnings). `netpol egress P require-ip` blocks their internet egress
  instead.
- **IPv6 egress IPs.** An egress IP may be IPv4, IPv6 or both
  (`ip HOST 'IPv4,IPv6'`); bpfd adds an `ip6 machina_egress` table.
- **Preview and approval for project changes.** `--preview` replays a
  change against the flow history; `--propose` (or
  `MACHINA_NETPOL_PROJECT_APPROVAL=1`) sends it to a second admin as a
  `vm_netpol.project` approval.
- **Project admins.** A Fleet Cloud project's admins may change its
  isolation, egress allowlist and `require-ip`; members may preview and
  export its evidence.
- **Evidence.** Fleet reports are signed (ECDSA P-256, certificate from the
  controller's netpol CA, verified by `netpol evidence verify`), take
  `--probes` and `--project`, list warnings, and are exported on a schedule
  (`MACHINA_NETPOL_EVIDENCE_DIR`, `…_EVERY_HOURS`, `…_KEEP_DAYS`) with an
  archive API and `netpol evidence archive`. Fleet-wide reports need an
  operator.
- **UI.** The *Projects* view assigns VMs to projects, shows warnings and
  egress gaps, has preview and approval toggles, a `require-ip` switch, and
  a per-project matrix with evidence export. Fleet Cloud instance pages show
  the instance's isolation and egress.

## 2026-10-04 — Fleet VM network policy fixes from real-VM testing

- **Host identity under the controller.** When the controller owns a host's
  VM edge, `machina-bpfd` now counts every global address of the node as
  `host`. Before, only the management address did, so traffic from libvirt
  bridge addresses was dropped by `fromEntities: host` rules. That affected
  SSH into isolated projects, DNS through the host, and pings to the gateway.
- **`netpol project assign P|- VM...`** puts VMs into a Fleet Cloud project
  from the CLI.
- **`machinactl --fleet netpol …`** works with the flag before the command,
  as the docs show (the flag after the command still works).
- **Daemon → controller proxy** forwards `Content-Type` and
  `Content-Disposition`, so controller downloads keep their type and
  filename.
- `scripts/bpf/vm-netpol-realvm.sh` gains a fleet phase on the same two VMs.
  It covers project isolation (cross-project dropped, same project and host
  allowed, `--no-host`), an egress allowlist (domain, IP and port, unlisted
  dropped), a project egress IP seen from a TEST-NET netns, and evidence.

## 2026-10-04 — Project isolation, egress control and segmentation evidence

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#project-isolation-fleet-cloud).

- **Project isolation.** Fleet Cloud projects can be isolated from each
  other with `machinactl --fleet netpol project isolate P`, or by default
  with `netpol project default isolated`.
  - An isolated project's VMs accept connections only from the same
    project and the host (unless `--no-host`). Ordinary policies still add
    exceptions.
  - Each setting generates a `project-isolation-…` policy that is traced,
    replayed and pushed like any other.
- **Egress allowlists.** `netpol egress P allow TO --port N` limits a
  project to CIDRs, IPs, domains, `*.domain` or `world`, plus its own VMs,
  the host and DNS. It generates a `project-egress-…` policy.
- **Per-project egress IPs.** `netpol egress P ip HOST IP` rewrites the
  source of the project's internet-bound traffic on that host.
  - `machina-bpfd` applies the rules as one atomic nftables table,
    `ip machina_egress`. Private and link-local destinations are never
    rewritten.
  - Addresses missing from the host are reported, not applied.
  - The table is saved and restored across bpfd restarts. Nothing is
    installed until an egress IP is set.
- **Segmentation evidence.** `netpol evidence` (UI: *Export evidence*)
  produces a SHA-256-sealed report for audits, as JSON or Markdown. It
  covers policies with hashes and selected VMs, host sync and enforcement,
  project settings, a reachability matrix traced from the policy set,
  denied connections, alerts, quarantines, temporary access, threat feeds
  and egress IPs.
  - On the fleet, it also includes 90 days of approvals.
  - `netpol evidence verify FILE` checks the digest.
- **UI.** A *Projects* view on the VM Network Policies page, with the
  default isolation, per-project isolation, egress allowlist and egress
  IPs, plus each host's egress state.
- **API.**
  - Controller: `GET /api/v1/vm-network-policies/projects`,
    `PUT`/`DELETE …/projects/{project}` and `GET …/egress-ips`.
  - Daemon and controller: `GET …/evidence[?format=md]`.
  - bpfd: `vm_egress_snat_set` / `vm_egress_snat_status`.

## 2026-10-04 — Plain-English VM network policies

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#plain-english-policies).

- **Drafts from sentences.** `machinactl netpol draft "only web servers can
  reach the db on port 5432"` prints policy YAML.
  - The draft is validated and replayed against the flow history; nothing
    is applied.
  - A built-in sentence parser handles *only … can reach*, *allow*, *block*
    / *must not* and *isolate*. Endpoints can be VMs, labels, host, the
    internet, CIDRs or domains, and ports can be service names.
  - The parser reports what it cannot read instead of guessing.
- **Zyvor.** On the controller, the configured LLM drafts first.
  - Its YAML must validate and select only existing VMs; it gets one repair
    round, otherwise the parser takes over.
  - In Zyvor chat, messages starting with `/policy` or `policy:` return a
    draft.
- **Approval.** On the fleet, `--propose` (*Request approval* in the UI)
  files a `vm_netpol.apply` action.
  - A second admin approves it, and the requester's own approval is
    refused.
  - On approval the YAML is re-validated, applied and pushed to every host,
    and recorded as `netpol.nl` events.
- **UI.** The YAML editor tab starts with a *Describe in plain English*
  box. It fills the editor, shows notes and the replay verdict, and lists
  drafts waiting for approval.
- **API.** `POST /api/v1/vm-network-policies/draft` on the daemon and the
  controller. The controller also has `GET …/draft` and
  `POST …/draft/propose`.

## 2026-10-04 — DNS threat feeds

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#dns-threat-feeds).

- **Threat feeds.** `machinactl netpol threat set NAME --url U` (or `-f
  FILE`, `--domain D`) checks every VM's DNS replies against a domain list.
  - It reads plain lists, hosts files and Adblock rules.
  - URL feeds are fetched again every 12 hours.
  - Feeds survive a bpfd restart.
- **Alerts and blocking.** A listed name raises a `threat_domain` alert.
  With `--block`, its answer addresses are denied as egress for every VM for
  the record's TTL: AUDIT in observe mode, dropped under the enforcement
  lease, with the reason `threat-domain`; these flows are logged even
  without flow logging. Addresses of known VMs are never blocked. On the fleet, a `threat_domain` alert also files a pending
  quarantine.
- **New-domain alerts.** Once the flow history is a day old, the first
  lookup under a base domain no VM has resolved before raises `new_domain`.
- **Fleet.** With `--fleet`, the controller stores the feeds and keeps every
  online host on exactly that set. Changes and refreshes become
  `netpol.threat` events.
- **UI.** A *DNS threat feeds* panel on the Alerts tab lists feeds, watched
  VMs and blocked addresses, and adds, refreshes or removes feeds.
- **API.** New `/api/v1/vm-network-policies/threat-feeds` routes on both the
  daemon and the controller.
- **Datapath plumbing.**
  - `machina-agent` now accepts gRPC requests up to 80 MiB, so large feeds
    reach remote hosts.
  - bpfd updates the edge settings of already-programmed taps in place when
    their flags change.

## 2026-10-04 — Just-in-time network access

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#just-in-time-access).

- **Temporary access.** `machinactl netpol jit grant --from web-1 --to db-1
  --port 5432 --for 1h` lets one VM (or `host`) reach another VM's port
  until the deadline (max 24 hours).
  - It only adds an allow (`enableDefaultDeny: false`), so it never
    isolates either VM.
  - The daemon removes it within a second of expiry; the controller within
    30 s.
  - `machina.io/expires-at` also works on hand-written policies.
- **Approval on the fleet.** Through the controller, a grant is a request in
  Approvals (`vm_netpol.jit`) that a second admin must approve; the
  requester's own approval is refused. `--now` lets an admin grant
  directly. Grants, requests and expiries become `netpol.jit` events.
- **UI.** A *Temporary access* panel on the Policies tab offers request or
  grant, pending requests with Approve and Reject, and active grants with
  time left and Revoke.
- **API.** New `GET`/`POST /api/v1/vm-network-policies/jit` route on both
  the daemon and the controller.
- **Fixes.**
  - `machinactl netpol status` printed the enforcement mode as raw JSON.
  - The real-VM test script now clears the flow history first, so a rerun
    no longer fails replay on traffic from the previous run.

## 2026-10-04 — VM quarantine

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#quarantine).

- **Quarantine.** `machinactl vm quarantine VM --for 1h` cuts every flow of
  a VM for a fixed time, up to 24 hours. That includes connections already
  open, in both directions, and it applies in observe mode too. Exceptions
  are optional: `--allow-host-ssh`, or `--allow ingress:host:tcp/22`.
  `vm release` lifts it early and `netpol quarantines` lists active ones.
- **Lifts itself.** The deadline lives in the datapath, so the quarantine
  ends on time even if bpfd or the daemon is down. bpfd also holds it again
  after a restart for the time left. VMs without any policy can be
  quarantined too.
- **UI.** The *Endpoints* tab has a Quarantine panel (duration, reason, host
  SSH) with a list and Release buttons, and a Quarantine button per VM.
  Alert rows have a Quarantine button for the source VM.
- **Fleet and Zyvor.** On the controller, a quarantine applies on every
  host, so it follows the VM through migrations. Each quarantine and
  release is recorded as a `netpol.quarantine` event. A high-severity port
  scan or host sweep proposes a `vm.quarantine` action in Approvals, which
  runs only once approved.
- New API routes on both the daemon and the controller: `POST`/`DELETE
  /api/v1/vms/{name}/quarantine` and `GET
  /api/v1/vm-network-policies/quarantines`.

## 2026-10-04 — Flow history, service map, learn, replay and alerts

See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md#flow-history-service-map-learn-replay-and-alerts).

- **Flow history.** bpfd keeps 7 days of VM flow edges, and saves them
  across restarts, with normalised L7 requests per edge. Proxied HTTP adds
  the response status class and latency. The host, other nodes and the
  internet appear by address, tagged with their entity.
- **Service map.** A new UI tab draws who talks to whom, coloured by verdict.
  Pick a link for its ports, policies and L7 rate, errors and latency.
- **Learn.** `netpol learn` and the *Learn* tab write least-privilege
  CiliumNetworkPolicy YAML from observed traffic. It uses VM selectors,
  `toEntities` for the host, `toFQDNs` or `/32` for external addresses, and
  HTTP, DNS and SNI rules.
- **Replay.** `netpol replay -f` and the editor's *Replay history* button
  show what a draft would have broken or newly allowed over the last 7
  days. The CLI exits non-zero if anything would break.
- **Alerts.** Port-scan, host-sweep, deny-burst and new-peer detection on
  each host. The controller sends new alerts out as `netpol.alert` events
  (webhooks, SIEM).
- New API routes on both the daemon and the controller: `GET`/`DELETE
  /api/v1/flows/edges`, `GET /api/v1/flows/alerts`, `POST
  …/vm-network-policies/learn` and `…/replay`. New CLI commands: `flow
  edges`, `flow alerts`, `flow reset`.

## 2026-10-04 — Stale VM inventory

- **Inventory pruning.** A host that reports no VMs on three consecutive
  inventory scans is now taken at its word. Previously an empty scan never
  pruned, so a host's last deleted VMs stayed listed as running forever and
  their consoles, keys and power actions failed.
- **Zyvor agent.** Troubleshooting flags a VM whose host no longer lists it.
  Autopilot proposes `remove_stale_vm` for such unmanaged VMs; approving it
  re-checks staleness before deleting the row.
- **Console.** A failed send-key now shows the real error. The old message
  blamed qemu-guest-agent, but keys are sent through libvirt and need no
  agent.

## 2026-10-04 — VM network policy and packet flows

Which VM may talk to which, in both directions, using the CiliumNetworkPolicy
schema. Enforced natively by `machina-bpfd` on each VM tap, so Cilium is not
needed. See [docs/ebpf/vm-network-policy.md](docs/ebpf/vm-network-policy.md).

- **Policies.**
  - Accepts `CiliumNetworkPolicy`, `CiliumClusterwideNetworkPolicy` and
    `VmNetworkPolicy` YAML.
  - L3/L4 support: endpoint selectors with expressions, ingress and egress,
    `ingressDeny`/`egressDeny` (deny wins), `from/toRequires`, CIDR sets with
    `except`, entities, port ranges, named ports, ICMP types and
    `enableDefaultDeny`.
  - Validation errors carry Cilium-style paths.
  - `toFQDNs` (`matchName` / `matchPattern` with Cilium wildcards) is
    enforced natively. The VM edge snoops DNS replies on the tap. bpfd gives
    each learned address its own identity, which inherits the rules of its
    CIDR or of `world`, so denies still win, and adds the FQDN allows. The
    learned names are listed by `machinactl netpol fqdn`, the UI
    *DNS names* table and `GET …/fqdn-cache`. The policy tester accepts DNS
    names.
  - L7 rules are enforced natively, without Envoy:
    - covers HTTP (method, path, host, headers), Kafka (role or apiKey,
      version, clientID, topic), TLS `serverNames` and DNS `matchName` /
      `matchPattern`, including L7 on `toFQDNs` rules;
    - the VM edge holds client segments past the allowed window; bpfd parses
      the stream (HTTP headers across segments, `Content-Length` and chunked
      bodies, keep-alive, HTTP/2 and gRPC with HPACK, whole Kafka requests,
      TLS SNI, DNS over UDP and TCP), then reinjects the allowed frames at once through a private veth
      (no retransmission wait), or answers with HTTP 403, a TCP reset or DNS
      REFUSED;
    - windows are per tap, so both ends of a VM-to-VM connection on one host
      are checked;
    - `toFQDNs` also learns from DNS answers over TCP;
    - flows carry the L7 request.
  - `toGroups` / `fromGroups` and `cidrGroupRef` resolve `CiliumCIDRGroup`
    objects.
  - `authentication` (`required`, `test-always-fail`): new connections need
    an authenticated VM identity pair. Peers on another host are
    authenticated by mutual TLS (TLS 1.3, port 4250) between the two
    hosts' bpfds, with host certificates the controller's CA signs from
    CSRs (keys stay on the hosts, 24-hour lifetime, renewed automatically).
    A source guard drops VMs that send as another VM's address.
    `machinactl netpol auth` and `GET …/vm-network-policies/auth` list the
    pairs; `netpol status` shows the host certificate.
  - `machinactl netpol test` and the UI tester take an L7 request.
  - `toServices` (`k8sService`, `k8sServiceSelector`) is enforced
    natively. The controller treats Fleet Cloud load balancers as services
    (listener plus members); the daemon reads Kubernetes services and
    endpoints through kubectl. Rule `toPorts` are intersected with the
    service ports.
  - `terminatingTLS`, `originatingTLS` and `headerMatches` with `ADD`,
    `DELETE`, `REPLACE` or a `secret` are enforced on egress through a
    transparent proxy in bpfd. Under the lease, the edge redirects the VM's
    TCP connection through the inject veth, and `bpf_sk_assign` hands it to
    bpfd. bpfd terminates and re-originates TLS with rustls, checks and
    rewrites each HTTP/1.x request, and connects upstream with the client's
    identity in its socket mark, so the server's policy still sees the
    client VM. Secrets are files on each host
    (`/etc/machina/netpol-secrets/<namespace>/<name>/`). `netpol status`
    shows the proxy.
- **Tests.** `scripts/bpf/vm-netpol-realvm.sh` checks observe and leased
  enforcement (L4 drops, L7 403s, flows) and the TLS-intercepting proxy
  (rewrites, 403, client identity at the server, `originatingTLS`) on two
  disposable libvirt VMs, and cleans up after itself. Playwright covers the policy page, with mocked and
  read-only live specs.
- **Labels.** Each VM has key/value labels, which policies select on. The
  daemon stores them in `vm-labels.json`. On the controller, migration 029
  adds a `vms.labels` column, seeded from `key=value` tags.
- **Single host or fleet.**
  - The daemon compiles policies for its own host and resyncs on lifecycle
    events and every 60 s.
  - The controller stores policies (migration 029) and pushes a per-host
    compiled state to every host's bpfd.
  - An owner field keeps the two writers apart.
- **Datapath.**
  - Identity-keyed rules with deny precedence and port ranges.
  - ICMP type rules.
  - CIDR identities through an LPM map.
  - ICMP echo conntrack keyed by identifier.
  - A ring buffer of FORWARDED, DROPPED and AUDIT flow events with rule
    attribution.
  - Observe by default; drops only under the enforcement lease.
- **Tools.**
  - `machinactl netpol` subcommands: `apply`, `get`, `delete`, `validate`,
    `test` (policy trace), `selectors`, `endpoints` and `status`.
  - `machinactl flow observe|top|stats`, with colours on a TTY.
  - `machinactl vm label`.
  - The same commands in `scripts/platformctl` against the controller.
- **UI.**
  - A **VM Network Policies** page: policies, YAML editor with dry-run
    preview and templates, policy tester, endpoints and selectors.
  - A black macOS-style **Flows** terminal.
  - A labels and policy panel on the VM's Network tab.

## 2026-10-04 — machina-cni is opt-in

- **Cluster bootstrap keeps the default CNI.** `POST /api/v1/k8s/cluster-bootstrap`
  now installs k3s with its bundled flannel, NetworkPolicy controller and
  kube-proxy and waits for nodes Ready. Pass `"cni": "machina"` (or tick
  **Use machina-cni** on the Kubernetes page) to get the previous behaviour:
  k3s without those three, plus `machina-bpfd` and `machina-cni`.
- **Takeover guard.** `machina-cni agent` exits with status 78 without
  touching the node when another CNI config is present in the CNI conf dirs;
  `contrib/machina-cni.service` has `RestartPreventExitStatus=78`. The
  bootstrap `cni` phase runs the same check. `MACHINA_CNI_TAKEOVER=1` overrides.

## 2026-10-03 — Native eBPF datapath (waves 1 and 2)

`machina-bpfd` replaces the Cilium, Tetragon, Netra and PacketWolf
integrations with Machina's own pure-Rust (Aya) programs. See
[docs/ebpf/](docs/ebpf/README.md).

- **Wave 1** — Policies with an observe default and a datapath-checked enforce
  lease (`MACHINA_BPF_ENFORCE_LEASE_SECS`, never persisted); flows, DNS, L7,
  per-VM accounting, captures, QoS and connection rate limits; `machina-cni`
  (dual-stack routing, NetworkPolicy, opt-in CiliumNetworkPolicy, Maglev
  services, XDP NodePort); VM edge and QEMU cgroup sandbox; XDP DDoS shield;
  TCP health and ICMP error histograms; opt-in TLS / JA4 visibility; node
  isolation with its own short lease. Daemon `/api/v1/bpf/*`, controller fleet
  views under `/api/v1/zeus-security/*` through agent `BpfCall`.
- **Wave 2** — Network-change audit (rtnetlink), sampled L7, VM runtime
  intelligence, VMM guard (BPF-LSM), direct tap redirect, QUIC-LB, AF_XDP and
  the `machina-scx` sched_ext VM scheduler. The Native eBPF page now has 22
  tabs.
- **Kernel features** — `GET /api/v1/bpf/status` reports `features` (`btf`,
  `tcx`, `cgroup2`, `fentry`, `lsm_bpf`, `sched_ext`, `xsk`) detected natively
  (no `bpftool` needed); the Overview tab shows them as pills. The `xsk` probe
  falls back to `/proc/kallsyms` because the unit's `RestrictAddressFamilies`
  blocks AF_XDP.
- **Guest policy relay** — `GET/PUT /api/v1/vms/{name}/guest-policy` and
  `/guest-lsm` relay per-container network and BPF-LSM rules to GuestKit's
  `guestkitd` through QGA guest-exec (allowlisted to four methods). New VM
  detail **Guest policy** tab.

## 2026-10-03 — Security fixes and regression TLS

- **CodeQL** — Verified TLS in the regression API helper, a URL scheme guard
  for controller-derived links (`safeHref`, http/https only), markdown
  escaping.
- **Dependabot** — `jsonwebtoken` 10, `async-nats` 0.50, `pam-client`, npm
  lockfile bumps.
- **Regression TLS** — `scripts/regression` verifies certificates by default.
  Set `MACHINA_CA_FILE` to trust a self-signed host CA, or
  `MACHINA_INSECURE_TLS=1` to skip verification on a lab host.

## 2026-10-03 — Docs refresh

- New `docs/ebpf/` reference, `docs/controller-ha.md`,
  `docs/daemon-peer-fleet.md` and `docs/handbook/runbook.md`; old fleet and
  runbook docs are stubs pointing to them. Snapshots moved to `docs/archive/`.
- One port table and full controller / agent / bpfd environment tables in the
  admin guide; Native eBPF troubleshooting.
- Website: networking category, intro, controller HA, upgrade and backup,
  troubleshooting, reference (ports, environment, API) and local search.
- Customer docs: Native eBPF, Guest policy and ten rewritten page guides; new
  routes (Alert Rules, Scheduled Jobs, Machine Finder, Native eBPF); feature
  guide PDF built by `build-customer-pdfs.mjs`.
- `scripts/check-doc-links.py` and a customer-routes freshness check run in CI.

## Earlier platform batches (moved from docs/platform.md)

- **Batch 12** — VM `lifecycle_phase`, structured API errors with
  remediation, host join validation, reconcile loop, live migrate after snap
  clone.
- **Batch 13** — Task drawer, command palette platform search,
  dashboard → platform link, structured error banners.
- **Batch 14** — Agent storage / network provisioning
  (`storage.pool.provision`, `network.provision`).
- **Batch 15** — Policy rules, project quotas, support bundle, upgrade
  manager, task-failure alerts.
- **Batch 16** — `scripts/platformctl`, Terraform stub under
  `terraform/machina/`, chaos / soak scripts.

## 2026-09-03 — Regression harness + host-sync 404 + LB DNAT -p

- **Stale host UUID** — Many `ops-*.js` / `ui-*.js` scripts defaulted to a
  retired lab host id, causing host-sync FK **500**s and catalog/mission 404s.
  Added `lib/ids.js` (`resolveIds`) and env-based `cfg.hostId` /
  `cfg.platformVmId` with live discovery after login.
- **`POST …/hosts/{id}/sync`** — Returns **404** when the host is missing
  instead of enqueueing a task that fails SQLite FK 787.
- **Native LB member push** — Backend DNAT rules now include `-p <protocol>`
  so nftables iptables accepts `--to-destination ip:port`.
- Soft-skip absent Zeus/Launchpad/KubeVirt routes; deepen `fleetcloud`
  (projects/stacks/templates lists + SG-only stack CRUD).

## 2026-09-03 — Fleet Cloud regression + port-forward delete

- **`npm run fleetcloud` / `make regression-fleetcloud`** — Native Fleet Cloud
  CRUD sweep (flavors, keypairs, security groups + rules, load balancers,
  port-forwards) via the daemon → controller proxy. Documented in
  `scripts/regression/RESULTS.md` (**25/25** on `212.8.248.187`).
- **Port-forward delete** — `core`/`machina-agent`: create used
  `-m comment --comment machina:…`, but delete issued a comment-less
  `iptables -D`, so the API returned `ok:true` while DNAT stayed. Deletes
  now use line numbers and fail if the rule remains. Agent rebuilt on lab.

## 2026-09-03 — Hotplug: PCI slots, honest disk detach, CD-ROM test

Live regression on `212.8.248.187` exposed three gaps after the TUI drop; all
retested green (`feature-test` **26/26**, `npm run lifecycle` **11/11**).

- **q35 NIC hotplug** — New domains get eight spare `<controller type='pci'
  model='pcie-root-port'/>` entries at create time. Existing VMs that hit
  “No more available PCI slots” on `POST …/nic/attach` now hot-add one root
  port and retry. Exhaustion maps to HTTP **409** `pci_slots_exhausted`
  instead of a bare 500.
- **Disk / NIC detach** — Live unplug wait extended to 8s; responses include
  `requires_restart` when `live_removed` is false (config already updated,
  guest hasn’t released the device). Regression lifecycle polls, then
  stop/start to apply config-only detach before asserting live XML.
- **feature-test CD-ROM** — Asserts the auto-picked target was not already
  occupied. Virtio-root Linux guests correctly receive free SATA `sda`; the
  old hardcoded `!= sda` check only fit Windows SATA-root VMs.

## 2026-08-15 — Firecracker: a third sprite backend

Sprites (`POST /v1/sprites`) can now boot on **Firecracker** as well as
libvirt/QEMU and Cloud Hypervisor — select via `backend: "firecracker"`.
Same disposable/TTL-reaped/destroy-only model as the other two backends
(`core/src/firecracker/`, mirrors `core/src/cloud_hypervisor/` closely
enough to diff side-by-side), with two real differences:

- **API-driven, not CLI-flag-driven.** `firecracker` starts serving only
  its control API over a Unix socket; boot config (vcpus/memory, kernel,
  drive, vsock, network interface) is a sequence of `PUT` calls, and the
  machine only actually boots on `PUT /actions {"action_type":"InstanceStart"}`.
  Each call shells `curl --unix-socket`, matching this project's existing
  "shell the CLI, don't link an HTTP client" convention.
- **Raw disk, no partition table.** Firecracker's drive backend is
  raw-only, and — a real bug found and fixed via a live boot, not
  assumed — it auto-appends `root=/dev/vda rw` (unpartitioned) for
  whichever drive has `is_root_device: true`, *after* whatever `boot_args`
  the caller supplies, so a caller-set `root=/dev/vda1` silently loses
  (kernel takes the last `root=` on the line; documented upstream as
  firecracker-microvm/firecracker#2709). Every golden image is a
  GPT-partitioned qcow2, so `materialize_raw_disk` now extracts partition
  1's content into an unpartitioned raw file (parsing `sfdisk -d`, `dd`-ing
  just that byte range) instead of handing Firecracker a whole partitioned
  disk — confirmed live: booting the full converted disk kernel-panicked
  with "Unable to mount root fs on /dev/vda"; booting the extracted
  partition mounts cleanly and boots straight through to a DHCP lease.
  Runs on every boot (no pre-extracted sibling file required), at a real
  cost — ~97s for the ~8.6 GB `debian-egress-test` golden image on this
  lab host, the slowest boot of the three backends.
- Firecracker itself needs no built-in BIOS/bootloader/qcow2 support the
  way the other two backends' quirks did — it boots a host-supplied kernel
  (`vmlinux`) directly. `install.sh`'s `ensure_firecracker` fetches the
  `firecracker`/`jailer` release tarball and a prebuilt `vmlinux` from
  Firecracker's own CI kernel bucket (the same source its getting-started
  guide uses) — optional, warn-and-continue on any fetch failure, same
  posture as `ensure_cloud_hypervisor`.
- `core/src/sprite_net.rs` (new) — promoted the TAP/bridge helpers,
  `SPRITE_RUN_DIR`, and VMM-binary discovery out of `cloud_hypervisor` into
  a shared module both backends now use, ahead of a third backend needing
  the same thing a third time.
- Jailer sandboxing (Firecracker's own chroot/cgroup/seccomp isolation) is
  deliberately out of scope for this pass — `firecracker` runs as a direct,
  unsandboxed daemon child, same posture Cloud Hypervisor already has.
  Documented as a future hardening item, not silently dropped.

Verified live end-to-end on a real host: real `firecracker`/`vmlinux`
install via `ensure_firecracker`, a real sprite boot against the fully
network-hardened `debian-egress-test` golden image (DHCP lease, SSH login,
`networkctl status`, `curl https://github.com` → `HTTP 200`), clean
teardown (process/TAP/run-dir all gone), automatic TTL reaping, and three
concurrent sprites — one per backend — drawing distinct vsock CIDs (3, 4,
5) from the same shared, backend-agnostic allocator with no collision.

## 2026-08-15 — Sprite fleet visibility, and golden-image networking hardening

**New: read-only sprite fleet visibility**, without reversing sprites'
deliberate exclusion from the controller's SQLite `vms` table/reconciler
(see `daemon/src/sprite_registry.rs`'s doc comment — sprites stay
TTL-reaped, disposable, and out of the reconcile-latency path).

- `machina-agent` gained a `ListSprites` gRPC method: it pulls its
  co-located daemon's `GET /api/v1/sprites` over loopback and relays the
  result, authenticated with a short-lived, read-only ("viewer") platform
  JWT it mints itself (`agent/src/jwt.rs`). The daemon now accepts platform
  JWTs from either `machina-controller` or `machina-agent` as issuer
  (`daemon/src/auth.rs`) — both rely on the same `MACHINA_JWT_SECRET`
  operators must already provision consistently for the existing KubeVirt
  inventory sync to work.
- The controller's existing per-host `host.inventory` task now also pulls
  sprite inventory (best-effort — an agent that predates `ListSprites`, or
  whose co-located daemon is down, doesn't fail the whole inventory tick)
  into a new in-memory-only cache (`controller/src/engine/sprite_inventory.rs`)
  — never written to `pool`/`vms`, so a controller restart just starts the
  cache empty again until the next tick repopulates it.
- `GET /api/v1/sprites` on the controller — fleet-wide sprite listing
  (optionally `?host_id=`), backed entirely by that cache, so it never
  blocks on a slow/unreachable host.

**Golden-image networking, baked in instead of patched live:** the
`debian-egress-test` golden image only had its DNS fix and `curl` applied
to a *running* sprite's overlay disk, not the base image — every future
sprite booted from it would still lack both. Rebuilt directly into the
base image this time, and expanded well past `curl`: a netshoot-equivalent
network-debugging toolkit (`ping`, `dig`, `traceroute`, `mtr`, `nc`,
`tcpdump`, `nmap`, `socat`, `telnet`) — because a disposable sandbox VM
with only `curl` isn't much of a debugging environment.

- **`guestkit rescue -o install-packages`** (new, upstreamed to the
  `guestkit` project): bind-mounts `/proc`,`/sys`,`/dev` into the mounted
  guest root (reusing `grub_repair`'s chroot machinery) and runs
  `apt-get`/`dnf`/`apk`/`pacman` inside via chroot — `--network`
  temporarily swaps the guest's `/etc/resolv.conf` for the host's so the
  package manager can resolve real repositories, restoring the original
  file afterward regardless of outcome. `virt-customize`'s network backend
  (`passt`) is broken on this lab host, which is exactly the class of
  problem this avoids — no libguestfs appliance network stack involved.
  Verified live: installed `jq` into the real golden image, confirmed it
  runs on next boot.

## 2026-08-15 — Sprites: network egress, machinactl, and Cloud Hypervisor fixes found live

Follow-on to the Cloud Hypervisor sprite backend below — everything here
came out of actually running the feature end-to-end rather than unit tests
alone.

**Fixed, from real boot failures:**
- Cloud Hypervisor has no built-in BIOS (unlike QEMU) — booting a disk
  without `--firmware`/`--kernel` failed immediately. `install.sh` now
  fetches `CLOUDHV.fd` from `cloud-hypervisor/edk2` releases alongside the
  binaries; `core::cloud_hypervisor` resolves it the same way it resolves
  the VMM binary itself.
- Cloud Hypervisor's qcow2 backend rejects backing-file overlays outright
  (`MaxNestingDepthExceeded`), even one level deep — the `cloudhypervisor`
  backend now materializes a full `cp --reflink=auto --sparse=always` copy
  instead (near-instant on reflink-capable filesystems, a plain copy
  otherwise — both cheaper than the alternative `qemu-img convert -c`,
  which compresses every cluster).
- Two independent vsock CID allocators (the daemon's own counter for Cloud
  Hypervisor, libvirt's kernel-side `<cid auto='yes'/>`) both started at
  CID 3 and collided the first time each backend's first sprite booted
  around the same time. `SpriteRegistry` now tracks CIDs from both
  backends in one shared set.
- stderr was previously discarded (`Stdio::null()`) — now piped to
  `tracing::warn!` continuously, which is what made the two boot-failure
  bugs above slow to diagnose in the first place.

**New:**
- `network_egress` (opt-in, off by default) — attaches a sprite to the
  host's existing libvirt "default" NAT network instead of staying
  vsock-only. Libvirt sprites get a `<interface type='network'>`; Cloud
  Hypervisor sprites get a TAP device created and bridged by the daemon.
  Verified live via a real DHCP lease, not just the TAP/bridge plumbing —
  see below.
- Sprites web UI (`/sprites`) — create modal (golden image picker, backend
  toggle, TTL, network egress checkbox), live list with expiry countdown,
  delete.
- `machinactl sprite <list|get|create|delete|golden-images>` — CLI parity
  with the API/UI, `MACHINA_API_TOKEN` for auth.
- `GET /v1/sprites/golden-images` — lists available golden images, backing
  the picker above.

**Verification gap closed:** the `network_egress` feature's own unit tests
use a blank synthetic disk (proves the TAP/bridge/NAT plumbing works, not
that a guest can actually get an address). Building a real test golden
image surfaced a second, unrelated bug: `virt-builder`'s plain templates
bake in a build-time-specific predictable interface name (e.g. `ens2`)
that doesn't match a sprite's actual device topology, so `ifupdown` never
brings the interface up. Fixed by switching the test image to
`systemd-networkd` with a `Name=en* eth*` wildcard match instead of a
hardcoded name. `scripts/sprite-verify-egress.sh` now automates the whole
check (create via the real dashboard, poll `virsh domifaddr`, fail loudly
with the sprite left running for inspection if no address appears) —
confirmed passing, DHCP lease in 9s. `scripts/sprite-remote-test.sh`
codifies the "fix `target/` ownership, run tests as root so the live
cloud-hypervisor boot tests actually run" sequence that was otherwise
hand-typed over SSH throughout this work.

## 2026-08-14 — Cloud Hypervisor backend for disposable "sprite" VMs

Sprites (`POST /v1/sprites` — instant, TTL-reaped, headless sandbox microVMs,
see `spec/src/sprite.rs`) can now boot on **Cloud Hypervisor** as an
alternative to the original libvirt/QEMU backend, selected per-request via a
new `backend` field (`"libvirt"`, the default, or `"cloudhypervisor"`).

- `core/src/cloud_hypervisor/` (new) — boots `cloud-hypervisor` as a direct
  child process of `machina-daemon` (there's no libvirtd in this path),
  reusing the existing golden-image qcow2-overlay registry
  (`core::libvirt::sprite::resolve_golden_image`,
  `core::libvirt::template_apply::materialize_from_base`). Teardown shells
  `ch-remote shutdown-vmm`, falling back to `SIGKILL` — same destroy-only
  semantics the libvirt backend already uses, no ACPI-graceful shutdown
  attempted.
- `daemon/src/sprite_registry.rs` — the in-memory registry/TTL reaper is now
  backend-agnostic (`SpriteBackendHandle::{Libvirt, CloudHypervisor}`) and
  hands out host-wide-unique vsock guest CIDs across both backends (Cloud
  Hypervisor requires an explicit CID, unlike libvirt's `<cid auto='yes'/>`,
  and CIDs are arbitrated by the kernel regardless of hypervisor).
- `daemon/src/routes/sprites.rs` — `create_sprite` dispatches on
  `req.backend`; `list`/`get`/`delete` are unchanged.

Deliberately out of scope for this pass: network egress
allow-list integration (Cloud Hypervisor sprites stay vsock-only, matching
today's libvirt sprites), a Kubernetes CRD/operator wrapper, a pluggable
disk-backend abstraction, and multi-host scheduling through the controller.

Verified with `cargo test -p machina-spec -p machina-core -p machina-daemon`
(25 passing: 5 new/updated in `core`, 13 in `spec`, 7 in `daemon`) plus a
full `install.sh` deploy on two Linux hosts (one redeploy onto an existing
install, one from-scratch). Not yet verified: a live `cloudhypervisor`-backend
sprite boot/teardown against a real `cloud-hypervisor` install (neither test
host has the binary installed).

## 2026-07-23 – 2026-07-25 — Security & correctness hardening marathon

Over three days, a multi-wave audit swept the entire Machina codebase — the full
Rust workspace (`core`, `daemon`, `controller`, `agent`, `tui`, `spec`,
`translate`, `rvb`, `virt-image-build`), the web frontend, ~90 shell/deploy
scripts, SQL migrations, CI, the Kubernetes/Helm chart, Docker, and the
Windows/Linux golden-image pipelines. **360+ discrete bugs** were found and
fixed across **19 waves**, each independently verified (`cargo check`/
`cargo test --workspace` on a Linux build host, `tsc --noEmit`/`vitest run`
for web changes, `bash -n`/`helm lint` where applicable) before merging.

Four dedicated self-review passes (waves 14/16/17/18/19) re-audited prior
waves' own fixes rather than hunting new ground, and found a real,
verifiable gap in **every single pass** — most notably an HA-recovery
`host_id`-revert compensation path (added in wave 14) that took **five
follow-up waves** to fully close off across seven independent bypass
routes (task retry, task cancel, task-bus republish failure, and two
startup crash-reapers).

### Highlights (most severe findings)

- **Sendmail flag injection** (wave 19) — a notification-channel email
  address, fully attacker/operator-controlled and validated only for `@`
  and no whitespace, reached `sendmail` as a bare positional argument. A
  value like `-C/tmp/evil.cf@x` was a working flag-injection primitive.
- **HA-recovery `host_id` never reverted on failure** (waves 14, 16, 17,
  18, 19) — an HA failover writes the destination `host_id` before its
  recovery task exists, so any way to fail/cancel/retry/crash out of that
  task without reverting it permanently stranded the VM's control-plane
  record. Closed across 7 independent code paths.
- **Zeus firewall `force`-flag authz gap** (wave 12) — bypassing the
  firewall-change approval gate required only `operator`, unlike every
  sibling "skip the safety gate" action in the same file.
- **Fail-open migration/health prechecks** (wave 16, confirmed a prior
  flag) — an unreachable source host or guest agent during a migration
  precheck or VM health check silently produced a passing/healthy result
  instead of blocking or flagging degraded state.
- **Host-cockpit privilege escalation** (wave 19) — an operator-gated
  endpoint forwarded an unrestricted action string to the same agent RPC
  its sibling endpoint correctly gates behind `require_admin`, reaching
  `storage.pool.delete`/`storage.volume.delete`/`network.delete`.
- **Unauthenticated plaintext-credential leaks** (waves 15, 19) — VM
  domain-XML (embeds VNC/SPICE passwords), host process lists (leak
  secrets via `/proc/<pid>/cmdline`), and RBAC role enumeration were all
  reachable with no role check, unlike their guarded siblings.
- **Atlas backup/snapshot/restore false success** (wave 14) — task
  handlers marked DB records `'completed'` the instant Atlas *accepted*
  a job (202), without polling to a terminal state; a failed Atlas-side
  operation silently reported success.
- **Zeus firewall temporary rules never enforced** (waves 17/18) — a
  "break-glass" temporary rule was recorded as `applied: true` and shown
  as live/auto-expiring, but no code path ever pushed it to a host
  firewall. Now honestly reported as audit-only.
- **CSV/YAML export injection** (waves 15/16) — exported VM/team/cost
  names starting with `=`/`+`/`-`/`@` could execute as a spreadsheet
  formula on open (CWE-1236); YAML exports interpolated names unescaped.
- **Helm chart hardcoded JWT secret** (wave 16) — `values.yaml` shipped
  `jwtSecret: "change-me-in-production"` as a literal default, which the
  daemon (unlike its own known dev secret) would accept as genuine,
  letting anyone forge admin tokens on an unmodified deployment.
- **Command-injection in remote deploy/install scripts** (waves 13, 19) —
  unquoted variables spliced into root SSH command strings
  (`install.sh --bind`, `deploy-remote.sh`'s license-key export), and a
  systemic sweep of every `Command::new`/`virsh`/`systemctl`/`nmcli`/
  `sendmail` call site for the "leading `-` as flag injection" pattern.
- **Unverified golden-image supply chain** (wave 15) — the Ubuntu
  desktop golden-image build downloaded the base cloud image over HTTPS
  with no checksum verification before using it as every VM's base.
- **Graceful shutdown gap** (wave 15) — the daemon's TLS (production)
  listener never wired up graceful shutdown; SIGTERM hard-killed
  in-flight VNC/SPICE/SSH console sessions instead of draining them.
- **OIDC hardening** (wave 18) — no timeout on IdP HTTP calls (could
  hang auth handlers indefinitely), and no issuer-pinning check on the
  fetched discovery document (OIDC Discovery 1.0 §4.3).
- **RCE in e2e test infrastructure** (wave 11) — a guest-exec helper's
  mismatched shell-escaping allowed command injection when a test VM's
  guest script contained an apostrophe.
- **Live VNC console regression** (self-caught same day) — a bundled
  vs. system noVNC version mismatch broke console fallback; found via
  live browser verification and fixed within the same session.

### Wave-by-wave summary

| Wave | Date (UTC-ish) | Commit | Fixes | Focus |
|---|---|---|---|---|
| — | 2026-07-23 21:06 | `23f51dbd` | — | Pre-marathon: closed daemon/controller authz gaps, hardened secrets, fixed task-delivery bugs |
| 1 | 2026-07-24 07:04 | `0df82f91` | 65 | Broad sweep: daemon, controller, core, agent, tui, web |
| 2 | 2026-07-24 08:19 | `18caf8a9` | 55 | Second broad wave: controller, daemon, spec, web |
| 3a | 2026-07-24 14:38 | `fbe6fc3d` | 3 | Partial: URL-encoding gaps, CSV injection |
| 3 | 2026-07-24 15:49 | `ffcf7e13` | 19 | Agent crate, web api/utils, deploy scripts |
| 4 | 2026-07-24 18:17 | `f4ae5c36` | 22 | Dedicated re-review of highest-value files |
| 5 | 2026-07-24 18:40 | `32acc7e1` | 4 | Cross-file and cross-layer deep reviews |
| 6 | 2026-07-24 19:08 | `a15089e3` | 6 | Security headers, rate limiting, systemd hardening, deps |
| — | 2026-07-24 20:13 | `e6ae7591` | 1 | Live VNC console regression hotfix |
| 7 | 2026-07-24 20:56 | `36a0ffbe` | 8 | Daemon rate limiting, cookie hardening, second passes |
| 8 | 2026-07-24 22:12 | `91e00ab8` | 17 | CI, K8s chart, Docker, Windows golden image, polkit |
| 9 | 2026-07-24 23:26 | `81a70da7` | 13 | K8s/observability configs, example configs, docs drift |
| 10 | 2026-07-25 00:02 | `2a165be2` | 15 | machinactl, privileged scripts, e2e test quality |
| 11 | 2026-07-25 00:12 | `87068167` | 6 | Remaining e2e/bundle scripts, incl. one real RCE |
| 12 | 2026-07-25 00:34 | `7748e958` | 1 | Zeus firewall force-flag authz gap |
| 13 | 2026-07-25 11:21 | `27b526eb` | 21 | AI engine, agent, TUI, spec/translate, deploy scripts |
| 14 | 2026-07-25 12:29 | `1a7cf65b` | 16 | HA/DRS, task bus, JWT auth, daemon routes, Atlas, bootstrap race |
| 15 | 2026-07-25 13:50 | `a529d49f` | 18 | API authz, agent RPC timeouts, graceful shutdown, supply chain |
| 16 | 2026-07-25 14:12 | `cf641483` | 14 | Fail-open precheck bugs, Helm secret default, CSV/YAML injection, KubeVirt false success |
| 17 | 2026-07-25 14:47 | `f07077e1` | 17 | Core crate first pass, console session lifecycle, HA revert gaps |
| 18 | 2026-07-25 15:36 | `145ddb4d` | 11 | 6th HA-revert bypass, OIDC hardening, firewall honesty, glass/hooks |
| 19 | 2026-07-25 18:27 | `ae8e608e` | 34 | Sendmail flag injection, 7th HA-revert bypass, host-cockpit privilege escalation, systemic argv sweep |

Fix counts are as documented in each wave's commit message; the session's
own running tally (quoted informally as work progressed) landed a little
higher (~385) after accounting for extra fixes self-review passes found
inside a wave that weren't reflected in that wave's headline count.

For full technical detail on any fix, see the corresponding commit message
(`git show <hash>`) — each documents the specific bug, the failure
scenario, and the fix applied.
