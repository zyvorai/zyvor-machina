---
sidebar_position: 2
title: Environment variables
description: Controller, agent, eBPF and CNI environment variables.
---

# Environment variables

The daemon is configured in `config.toml` ([Configuration](../operations/configuration.md)). The other services read
environment variables from their env files.

## Controller (`/etc/default/machina-platform`)

| Variable | Default | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | `sqlite:///var/lib/machina/controller.db` | Embedded SQLite state; no external database |
| `NATS_URL` | unset | Optional NATS task fan-out across controller instances |
| `MACHINA_CONTROLLER_ID` | random `ctrl-xxxxxxxx` per start | Stable instance id; set it on every instance in a multi-controller deployment |
| `MACHINA_AGENT_ADDR` | `http://127.0.0.1:50051` | Agent gRPC address |
| `MACHINA_AGENT_CA`, `MACHINA_AGENT_CLIENT_CERT`, `MACHINA_AGENT_CLIENT_KEY` | unset | mTLS to agents |
| `MACHINA_JWT_SECRET` | random per process | JWT signing secret; set it for sessions that survive restarts |
| `MACHINA_ALLOW_DEV_SECRETS` | unset | Allow the well-known development secret (never in production) |
| `MACHINA_API_KEY_MASTER_KEY` | unset | 64-hex-char AES-256-GCM key for stored LLM provider and BMC secrets (`openssl rand -hex 32`) |
| `MACHINA_API_KEY_MASTER_KEY_FILE` | unset | Path to a root-readable file holding that key; read first, so the key stays out of the environment |
| `MACHINA_JWT_PREVIOUS_SECRETS` | unset | Comma-separated earlier signing secrets, accepted for verification only (rollover) |
| `MACHINA_SECURITY_PROFILE` | `development` | `production` enables the strict preflight ([Production hardening](../operations/production-hardening.md)) |
| `MACHINA_AUDIT_RETENTION_DAYS` | `365` | Days of `audit_logs` kept; clamped to 7-3650, `0` keeps everything |
| `MACHINA_BPF_REQUIRED` | unset (set `true` by the packaged bpfd unit) | bpfd refuses to start without its embedded eBPF object |
| `MACHINA_TRANSIVA_URL`, `MACHINA_TRANSIVA_API_KEY`, `MACHINA_TRANSIVA_TIMEOUT_SECS` | `http://127.0.0.1:5070`, unset, `60` | [Transiva migration gateway](transiva-api.md) |
| `MACHINA_PUBLIC_URL` | `http://127.0.0.1:5093` | Public controller URL |
| `MACHINA_WEB_URL` | `http://127.0.0.1:5173` | Web UI URL used in links |
| `MACHINA_DAEMON_URL` | — | How the controller reaches the daemon |
| `MACHINA_FENCE_COMMAND` | unset | Fallback fence command run by the agent (`{hostname}` is substituted) |
| `MACHINA_BPF_ENFORCE_LEASE_SECS` | `900` | Default eBPF enforcement lease |
| `MACHINA_RATE_LIMIT_ENABLED`, `MACHINA_RATE_LIMIT_PER_MIN` | — | API rate limiting |
| `MACHINA_SMTP_HOST`, `_PORT`, `_USER`, `_PASS`, `_FROM` | unset | Email notifications |
| `MACHINA_AI_DISABLED` | unset | Turn off Zyra AI |
| `MACHINA_SKIP_AUTH` | unset | Disable auth (development only) |
| `GUESTKIT_ENABLED` | `true` | GuestKit features |
| `ATLAS_ENABLED`, `ATLAS_BASE_URL`, `ATLAS_TOKEN`, … | off | Atlas storage ([Integrations](../core-concepts/integrations.md)) |

## Agent

| Variable | Default | Purpose |
| --- | --- | --- |
| `MACHINA_LIBVIRT_URI` | `qemu:///system` | libvirt connection |
| `MACHINA_AGENT_TLS_CERT`, `MACHINA_AGENT_TLS_KEY` | unset | Agent TLS certificate |
| `MACHINA_AGENT_TOKEN` | unset | Shared token between controller and agent |
| `MACHINA_BPFD_SOCK` | `/run/machina-bpf/bpfd.sock` | Where to reach `machina-bpfd` |
| `MACHINA_FLUXVM_URL` | the host's `[fluxvm] base_url` when enabled | fluxvm-api the controller manages FluxVM VMs through |
| `MACHINA_FLUXVM_TOKEN` | `[fluxvm] token` | Bearer token for it |
| `MACHINA_FLUXVM_INSECURE_TLS` | `[fluxvm] insecure_tls` | `1` accepts a self-signed fluxvm-api |

## eBPF (`/etc/default/machina-bpfd`)

| Variable | Default | Purpose |
| --- | --- | --- |
| `MACHINA_BPFD_SOCK` | `/run/machina-bpf/bpfd.sock` | Control socket |
| `MACHINA_BPFD_STATE_DIR` | `/var/lib/machina/bpf` | Persisted accounting |
| `MACHINA_BPFD_SOCKET_GROUP` | empty (socket 0600) | Group allowed to use the socket |
| `MACHINA_BPF_ENFORCE_LEASE_SECS` | `900` | Enforcement lease length |
| `MACHINA_BPF_SOCKOPS_CGROUP` | `/sys/fs/cgroup` | cgroup for TCP connect timing |
| `MACHINA_BPF_TLSFP_CGROUP` | `/sys/fs/cgroup` | cgroup for TLS fingerprints |
| `MACHINA_BPF_L7S_CGROUP` | `/sys/fs/cgroup` | cgroup for sampled L7 |
| `MACHINA_BPF_XDP_SKB` | unset | Force generic (SKB) XDP mode |
| `MACHINA_SCX_BIN` | next to `machina-bpfd` | `machina-scx` helper path |

## Kubernetes CNI

See [Kubernetes CNI](../networking/kubernetes-cni.md#configuration) for `MACHINA_CNI_*`.

## Bare metal

The bare-metal provisioning variables (`MACHINA_BMC_*`, `MACHINA_METAL_*`, `MACHINA_IPMITOOL`) are listed with their
defaults in [Bare-metal API and CLI](bare-metal-api.md#configuration).
