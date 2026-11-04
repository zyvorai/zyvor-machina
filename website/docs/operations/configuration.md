---
sidebar_position: 1
title: Configuration
description: Daemon config.toml, controller environment variables and install paths.
---

# Configuration

## Daemon: `config.toml`

The daemon reads, in order:

1. `--config <path>` if given, otherwise
2. `/etc/machina/config.toml`, then `~/.machina/config.toml`, otherwise
3. built-in defaults.

CLI `--host`, `--port` and `--libvirt_uri` override the loaded values. Every field is optional. The shipped template
is `contrib/machina.toml`.

```toml
[daemon]
host = "0.0.0.0"
port = 5092

[libvirt]
uri = "qemu:///system"

[auth]
pam_service = "sshd"

[auth.oidc]
enabled = false
issuer_url = "https://sso.example.com/realms/ops"
client_id = "machina"

[tls]
enabled = true
cert_path = "/etc/machina/ssl/cert.pem"
key_path = "/etc/machina/ssl/key.pem"

[backup]
backup_dir = "/var/lib/machina/backups"
```

Other sections: `[fleet]` (peer daemons), `[vessel]` (Podman/Docker socket), `[metrics_history]`,
`[observability.otlp]` (see [Observability](observability.md)) and `[fluxvm]` (`enabled`, `base_url`, `token` or
`token_file`, `insecure_tls`, `default_backend`; see [FluxVM backend](../core-concepts/fluxvm.md) and the
[operator guide](https://github.com/zyvorai/zyvor-machina/blob/main/docs/fluxvm.md)).

## Controller: environment

Set in `/etc/default/machina-platform`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | `sqlite:///var/lib/machina/controller.db` | Embedded SQLite state |
| `NATS_URL` | unset | Optional NATS task fan-out |
| `MACHINA_AGENT_ADDR` | `http://127.0.0.1:50051` | Agent gRPC address |
| `MACHINA_JWT_SECRET` | random per process | JWT signing secret; set it for stable sessions |
| `MACHINA_PUBLIC_URL` | `http://127.0.0.1:5093` | Public controller URL |
| `MACHINA_API_KEY_MASTER_KEY` | unset | AES-256-GCM key for LLM provider keys |
| `MACHINA_CONTROLLER_ID` | random per start | Stable id; required per instance when running several controllers |
| `MACHINA_BPF_ENFORCE_LEASE_SECS` | `900` | Default eBPF enforcement lease |

Full list: [Environment variables](../reference/environment.md).

## eBPF: `/etc/default/machina-bpfd`

`machina-bpfd` runs as root from `machina-bpfd.service` and reads its overrides from `/etc/default/machina-bpfd`
(socket path, state directory, the lease length and the cgroups used for TCP, TLS and L7 sampling). Most features are
configured at runtime through the API or the **Native eBPF** page, not in files, and enforcement settings are never
persisted. See [Native eBPF](../networking/ebpf-overview.md).

## Install paths

| Artifact | Path |
| --- | --- |
| Binaries | `/usr/local/bin/machina-{daemon,controller,agent,bpfd,cni}` |
| Config | `/etc/machina/config.toml` |
| Env files | `/etc/default/machina-daemon`, `/etc/default/machina-platform`, `/etc/default/machina-bpfd` |
| systemd units | `/usr/lib/systemd/system/machina-*.service` |
| Web UI | `/usr/local/share/machina/web/` |
| State | `/var/lib/machina/` (controller database `controller.db`, eBPF accounting in `bpf/`) |
| eBPF socket | `/run/machina-bpf/bpfd.sock` |

The full reference lives in the
[admin configuration handbook](https://github.com/zyvorai/zyvor-machina/blob/main/docs/handbook/admin-configuration.md).
