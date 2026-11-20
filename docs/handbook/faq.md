# Machina — FAQ

*Part of the [Machina Handbook](README.md) · see also
[Product Guide](product-guide.md) · [Admin & Configuration](admin-configuration.md) ·
[Troubleshooting](troubleshooting.md)*

---

## Bare-metal provisioning

**Can Machina install machines that have no OS yet?**
Yes, from a BMC address to a joined host: it discovers or imports the BMCs, commissions
the machines over the network (or BMC virtual media), keeps a pool by zone, writes a
checksum-verified OS image (Ubuntu, Debian, Rocky, AlmaLinux) to the right disk, and the
machine joins on first boot with its site, tags, networks and pools. Guide:
[baremetal-provisioning.md](../baremetal-provisioning.md).

**Does it need PXE?**
No. PXE (proxyDHCP + TFTP beside your DHCP server) is one way; a Redfish BMC can mount a
small iPXE ISO as virtual media instead, which needs only DHCP on the machine's network.

**Is it tested on real servers?**
Not yet. The flow has run end to end on a lab host against VMs and BMC emulators (sushy-tools,
VirtualBMC). Validate on your hardware before relying on it; `docs/claims.md` lists exactly
what ran.

**Where are BMC passwords kept?**
Encrypted at rest (AES-256-GCM) when `MACHINA_API_KEY_MASTER_KEY` is set, never returned by
the API, and passed to `ipmitool` through the environment.

---

## Product & concepts

**Q1. What is Machina?**
An enterprise Linux hypervisor management platform — a unified control plane for
VMs, networks, storage, snapshots and day-two operations on bare-metal hosts,
built on libvirt/QEMU/KVM. It exposes a web UI, a REST API and
the `machinactl` CLI.

**Q2. Where does Machina fit in the Zyvor stack?**
It owns the metal: KVM hosts, their VMs, networking and storage, plus the
self-service Fleet Cloud on top. Sibling products plug in: Atlas (Ceph/NFS/ZFS
storage) and GuestKit (guest agent, in-guest eBPF policy, disk inspection).
Zeus OS (v9s) is a KubeVirt-based cloud that can run on Machina hosts. Network
enforcement and visibility are native (`machina-bpfd`), replacing Cilium,
Tetragon, Netra and PacketWolf.

**Q3. Is it a replacement for `virsh`?**
It sits above libvirt and replaces ad-hoc `virsh` scripting with one API + UI +
CLI. You can still use `virsh` directly against the same host — Machina reads and
writes standard libvirt domain XML.

**Q4. What's the difference between the daemon and the controller?**
`machina-daemon` (:5092) manages a **single host** and is the original product.
`machina-controller` (:5093) is the optional **multi-host** enterprise control
plane (fleet, HA, DRS, AI), talking to per-host `machina-agent` (:50051) over
gRPC. Many deployments run only the daemon.

**Q5. What languages/frameworks is it built with?**
Rust (Axum 0.8 / Tokio) for the daemon, controller and agent; React 19 + Vite +
TailwindCSS for the web UI.

---

## Ports & networking

**Q6. What port does Machina serve on?**
The daemon serves on **5092** (HTTPS by default). The controller is **5093**, the
agent gRPC is **50051** and its console proxy **50052**. The Vite dev server is
**3000**.

**Q7. How do I change the daemon port?**
Set `[daemon] port` in `/etc/machina/config.toml`, or pass `--port` /
`--host` on the command line (CLI overrides config). In Helm, set `daemon.port`.

**Q8. What does the daemon bind to by default?**
`0.0.0.0` (all interfaces). Front it with a firewall or reverse proxy, or set
`[daemon] host` to a specific address.

**Q9. Is HTTPS required?**
HTTPS is enabled by default with a self-signed cert. TLS is active only when
`[tls] enabled=true` **and** both `cert_path`/`key_path` are set — which the
installer does automatically.

---

## Authentication & access

**Q10. How do I log in? Is there a separate Machina password?**
No. Sign-in uses **PAM (Linux host accounts)** by default — the same credentials
as SSH on that host (`[auth].pam_service = "sshd"`). LDAP and OIDC SSO can be
enabled additionally.

**Q11. I can't log in as root over the web — why?**
The `login` PAM service is TTY-oriented and often blocks root. Machina defaults
to the `sshd` service instead; keep `pam_service = "sshd"` (or another service
that permits your account).

**Q12. What roles exist?**
`Admin`, `Operator`, `ReadOnly`. Admin can do everything (delete/destroy,
manage users, host paths); Operator can write and hot-plug but not delete;
ReadOnly can only view.

**Q13. How are roles assigned?**
Via `/var/lib/machina/roles.json` (username → role). **Important: if that file is
empty or missing, everyone is treated as Admin.** OIDC group mappings
(`admin_groups`/`operator_groups`) and API-token roles also apply.

**Q14. How does the session cookie work?**
On login the daemon sets an `HttpOnly; SameSite=Strict` cookie named
`machina_session` with a random token; server-side sessions last 24 hours.
The cookie also gets `Secure` whenever the daemon is actually serving over
TLS (the default, since HTTPS is on out of the box) — it's dropped only if
TLS ends up disabled/misconfigured, so it isn't sent in the clear over a
real HTTPS deployment.

**Q15. How do WebSocket consoles authenticate?**
Clients first `POST /api/v1/ws-token` to mint a single-use token, then connect to
`/ws/v1/...?token=<token>`.

**Q16. How do I create API tokens?**
API tokens are stored in `/var/lib/machina/api-tokens.json`, are prefixed
`mach_` or `vs_`, carry scopes, and map to a role. Note that API-token actors are
blocked from host-insight endpoints.

**Q17. Can I disable auth for local development?**
Yes — `MACHINA_DAEMON_SKIP_AUTH=1` on the daemon (or `MACHINA_SKIP_AUTH=1` on the
controller). **Never do this in production**; every request becomes admin.

---

## Deploying & running

**Q18. What's the fastest way to get running?**
On a Linux KVM host: `./machinactl deploy`. It does deps → build → install →
start → verify, then serve on `https://<host>:5092`.

**Q19. Can I build on macOS?**
No — the workspace needs Linux libvirt headers. Build on Linux or use
`./scripts/deploy-remote.sh USER@HOST --remote-build`. (The web UI alone builds
fine on macOS with `cd web && npm run build`.)

**Q20. How do I deploy to a remote host?**
`./scripts/deploy-remote.sh USER@HOST --remote-build` rsyncs and builds on the
remote. Add `--platform` for the controller/agent, or `--open-firewall`.

**Q21. Does it run as a systemd service or a container?**
Primarily **systemd** (`machina-daemon.service`, plus controller/agent/backup
units). A Helm chart at `contrib/k8s/machina/` can run the **daemon only** in
Kubernetes; the controller/agent tier is systemd-only.

**Q22. How do I update to a new version?**
`./machinactl upgrade` (git pull + reinstall) or `./machinactl reinstall` after
pulling. Config in `/etc/machina` and state in `/var/lib/machina` are preserved.

**Q23. How do I check that a deploy worked?**
`./machinactl verify` (smoke test) and `./machinactl health` (deep check — exit
0 healthy / 1 degraded / 2 critical). `./machinactl doctor` checks host
readiness before deploying.

**Q24. Where are binaries, config and state installed?**
Binaries in `/usr/local/bin/` (`machina-daemon`, `machina`), config in
`/etc/machina/config.toml`, web UI in `/usr/local/share/machina/web/`, state in
`/var/lib/machina/`.

---

## VMs, storage, networks

**Q25. How are VMs created — virt-install or raw XML?**
Both. `[libvirt] create_backend` chooses the default (`virt_install` or
`libvirt_xml`); clients can override per request. Golden images can be built
asynchronously via `POST /jobs/virt-image-build` (virt-builder / mkosi).

**Q26. Can I hot-plug disks/NICs and resize CPU/RAM?**
Yes — `POST /vms/{name}/disk/attach|detach|resize`,
`/nic/attach|detach`, `/vcpus/{count}`, `/memory/{mb}`, plus CPU pinning and
scheduler tuning.

**Q27. Does it support live migration?**
Yes — `POST /vms/{name}/migrate`, with `/migrate/max-bandwidth` and
`/migrate/max-downtime` tuning.

**Q28. What libvirt connection does it use?**
`qemu:///system` by default (`[libvirt] uri`). Set `dual_connection=true` to
connect both system and session, or `extra_uris` for additional hosts.

**Q29. How do snapshots and backups differ?**
Snapshots are per-VM libvirt snapshots (create/revert/delete). Backups are a
scheduled export driven by `scripts/backup.sh` and the `machina-backup.timer`
(daily 2 AM) into `[backup].backup_dir`, optionally to NFS, with retention.

**Q30. How do I enable scheduled backups?**
`./machinactl backup enable` (starts the daily timer). Check with
`./machinactl backup status`; run once with `./machinactl backup now`.

---

## Consoles & integrations

**Q31. How do console (VNC/SPICE/serial/SSH) sessions work?**
The daemon proxies them itself — noVNC (`/novnc` + `/ws/v1/vnc/{name}`), SPICE
HTML5 (`/spice-html5`), serial (`/ws/v1/console/{name}`), SSH terminal — so you
don't need a separate gateway. ConsoleHub brokers sessions with TTLs.

**Q32. Does it support RDP?**
Yes, natively for Windows guests. A Windows guest is auto-detected, and RDP is
exposed via a hypervisor NAT port-forward to the guest's port 3389 once the
agent confirms Remote Desktop is actually listening. Connect with a native
client — Microsoft Remote Desktop on macOS, or `mstsc` on Windows — using the
`.rdp` file generated from `GET /vms/{name}/rdp-info`.

**Q33. Can I migrate VMs to KubeVirt?**
Yes. Export a KubeVirt bundle (`GET /vms/{name}/kubevirt-bundle`) and
apply/upload/start it. See [../kubevirt-migration.md](../kubevirt-migration.md).

**Q34. Does Machina expose Prometheus metrics?**
Yes — `GET /api/v1/prometheus` is a scrape endpoint; there's a metrics-history
ring buffer, remote-write ingest, and OTLP/HTTP export
(`[observability.otlp]`) to Grafana Alloy / OTel Collector. See
[../guides/observability.md](../guides/observability.md).

**Q35. Is there an API contract I can generate clients from?**
Yes — `GET /api/v1/openapi.json`, also viewable in the UI at `/api-docs`.
Regenerate with `npm run generate-openapi` in `web/`.

---

## Zyvor stack

**Q36. What's the relationship to Zeus OS / v9s?**
Zeus OS is a separate KubeVirt-based cloud product. Machina can sit underneath
it (managing the physical KVM hosts) or stand alone with its own Fleet Cloud.
Machina's AI assistant is **Zyra AI** (`/platform/zyra`), backed by the
controller's AI engine.

**Q37. Does the controller need PostgreSQL?**
No — it uses embedded SQLite by default (`DATABASE_URL=
sqlite:///var/lib/machina/controller.db`), created and migrated on first
start, which is the right choice for 1 or 2 machines. For hundreds of
machines or several controllers on different hosts there is a PostgreSQL
build of the controller; `machinactl db setup pod|package|external` sets it
up (a managed Postgres in a Podman container, distribution packages, or your
own server). It passes the whole test suite against PostgreSQL 16 and has run
live in isolation (two controllers on one database, recovery after a restart),
but no site has run a real fleet on it yet. See
[the database guide](../guides/database.md). NATS (`NATS_URL`) is optional,
for fanning tasks out across several controller instances.

---

*Still stuck? See [Troubleshooting](troubleshooting.md).*
