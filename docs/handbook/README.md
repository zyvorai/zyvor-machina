# Machina Handbook

A consolidated, cross-linked handbook for **Machina** — the enterprise Linux
hypervisor management platform in the [Zyvor](https://zyvor.dev) stack.

> Machina turns bare-metal KVM hosts into a private cloud: VMs, consoles,
> networks (including a native eBPF datapath), storage, snapshots, fleet HA and
> self-service **Fleet Cloud**, built directly on **libvirt / QEMU / KVM** and
> managed from one UI, one REST API and `machinactl`.

---

## What Machina is (60-second orientation)

Machina replaces scattered `virsh` scripts and bolt-on console gateways with a
single control plane: a Rust daemon that speaks libvirt, plus a web UI, a
REST API, and the `machinactl` CLI. It ships built-in noVNC,
SPICE, serial and SSH console proxies, PAM/LDAP/OIDC auth with RBAC, Prometheus
metrics, VM backup, and a documented KubeVirt migration path.

```text
        ┌──────────── Interfaces ────────────┐
        │  Web UI · REST API · machinactl    │
        └──────────────────┬──────────────────┘
                           │ HTTPS :5092  (REST /api/v1 + WS /ws/v1)
                  ┌────────▼─────────┐
                  │  machina-daemon   │   PAM/LDAP/OIDC · RBAC · console proxies
                  │  (single host)    │
                  └────────┬─────────┘
                           │ libvirt
                  ┌────────▼─────────┐
                  │ libvirt/QEMU/KVM  │   same host
                  └──────────────────┘

  Optional multi-host control plane (systemd-only, installed with INSTALL_PLATFORM=1):

     machina-controller :5093  ──gRPC/TLS──►  machina-agent :50051 (console :50052)
      (fleet, HA, DRS, AI)                     (per hypervisor host)

  Native eBPF datapath (every host that wants it):

     machina-bpfd  ◄── /run/machina-bpf/bpfd.sock ──  daemon /api/v1/bpf/* · agent BpfCall · machina-cni
```

---

## Ports

The one authoritative port table; other docs link here.

| Port | Component | Protocol | Default | Configured by |
|------|-----------|----------|---------|---------------|
| **5092** | `machina-daemon` | HTTPS (REST `/api/v1` + WS `/ws/v1`) | bind `0.0.0.0:5092` | `[daemon]` in `config.toml`, `--host/--port`, Helm `daemon.port` |
| **5093** | `machina-controller` | HTTP/REST (multi-host) | `0.0.0.0:5093` | `machina-controller --host --port` |
| **50051** | `machina-agent` | gRPC (tonic, TLS) | `127.0.0.1:50051` | `machina-agent --listen` |
| **50052** | `machina-agent` console | WebSocket console proxy | `127.0.0.1:50052` | `machina-agent --console-listen` |
| **3000** | Web dev server (Vite) | HTTP | dev only | `web/vite.config.ts`, proxies `/api` + `/ws` to `:5092` |
| **4222** | NATS (optional, controller) | — | `NATS_URL` | controller env |
| socket | `machina-bpfd` | newline-delimited JSON over a Unix socket (root, 0600) | `/run/machina-bpf/bpfd.sock` | `MACHINA_BPFD_SOCK` |

TLS on `:5092` is **on by default** — the installer generates a self-signed cert
at `/etc/machina/ssl/cert.pem` (see [admin-configuration.md](admin-configuration.md#6-tls--certificates)).

---

## Fastest path to running

Machina builds on Linux only (needs libvirt headers). Do **not** build the Rust
workspace on macOS.

```bash
# On a Linux KVM host, as a user with sudo:
git clone https://github.com/zyvorai/machina.git && cd machina
./machinactl deploy          # deps → build → install → start → verify

# Then open the web UI (sign in with a Linux/PAM account on the host):
#   https://<host>:5092
```

Remote deploy from your workstation (builds on the remote Linux host):

```bash
./scripts/deploy-remote.sh USER@HOST              # rsync, build and install on the server
./scripts/deploy-remote.sh USER@HOST --platform   # also install the controller (:5093) + agent (:50051)
```

See the full matrix in [admin-configuration.md](admin-configuration.md#1-deployment--run-models).

---

## The guides

| Guide | What's inside |
|-------|---------------|
| **[Customer page-by-page manual](../customer/README.md)** | Learn every dashboard screen, with printable PDFs |
| [Product Guide](product-guide.md) | Concepts, every surface (CLI/API/UI), and feature deep-dives: VM lifecycle, storage, networks, snapshots, consoles, fleet, integrations. |
| [Administration & Configuration](admin-configuration.md) | Deploy/run models, ports, full `config.toml` + env-var reference, auth/RBAC, TLS, building from source, production checklist. |
| [FAQ](faq.md) | 30+ real questions on auth, ports, deploy, VMs, consoles, backups, integrations. |
| [Troubleshooting](troubleshooting.md) | Symptom-indexed fixes with real diagnostic commands (`machinactl health`, `journalctl`, `virsh`), including native eBPF. |
| [Bare-metal provisioning](../baremetal-provisioning.md) | BMCs to joined hosts: discovery, commissioning, OS image catalog, image deploy, profiles, virtual media, serial console, join labels; API/CLI reference, security model, troubleshooting, verification status. |
| [Operator runbook](runbook.md) | Procedures: install/upgrade, backup, sign-in, fleet playbooks (host down, HA, migration), eBPF enforcement and emergency isolation. |

---

## Related deep-dive docs

The handbook cross-links rather than duplicates these existing guides:

| Topic | Doc |
|-------|-----|
| Apple.com / Zeus UX contract | [../design/APPLE-UX-CONTRACT.md](../design/APPLE-UX-CONTRACT.md) |
| Daylight / Tahoe Light tokens (Apple shop box fonts) | [../design/DAYLIGHT-CONTRACT.md](../design/DAYLIGHT-CONTRACT.md) |
| UX author guide | [../ux.md](../ux.md) |
| KubeVirt migration | [../kubevirt-migration.md](../kubevirt-migration.md) |
| Observability | [../guides/observability.md](../guides/observability.md) |
| Native eBPF | [../ebpf/README.md](../ebpf/README.md) |
| Controller HA, fencing, DRS | [../controller-ha.md](../controller-ha.md) |
| Daemon peer fleet | [../daemon-peer-fleet.md](../daemon-peer-fleet.md) |
| User stories | [../USER_STORIES.md](../USER_STORIES.md) |

---

*Part of the Zyvor platform stack. See the [repo README](../../README.md) for the
full product ecosystem.*
