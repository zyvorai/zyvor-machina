# Package Machina as a Linux binary (remote build)

Deliver **machina-daemon**, **TUI**, and **web/dist** as a tarball—no `deploy-remote.sh` on the client machine. Build on a **Linux amd64 hypervisor-class host** (same distro family the client will run).

> **macOS developers:** do not run `make` or `cargo build` locally. Use this script (or `deploy-remote.sh --remote-build`) to compile on the remote host only. See [macos-build.md](macos-build.md).

## Quick command

```bash
./scripts/package-binary-remote.sh <ephemeral-ip> operator --fetch
./scripts/package-binary-remote.sh HOST USER --reuse-build --fetch   # skip make if already built
./scripts/package-binary-remote.sh <ephemeral-ip> operator --from-deploy --fetch   # tarball from ~/.deployment/machina
```

**`--from-deploy`** — use the tree already on the server after `deploy-remote.sh` (no `make release`); only refreshes packaging scripts from your laptop, then assembles the customer tarball.

## What is in the tarball

```
machina-0.1.0-linux-amd64/
  machina-daemon
  machina                 # TUI (if built)
  web/dist/
  machina.toml.example
  machina-daemon.service
  README.txt
```

## Prerequisites (build host)

| Tool | Purpose |
|------|---------|
| **Rust** + **cargo** | `make release` |
| **Node/npm** | `make web` |
| **libvirt dev** | Links `machina-daemon` (see `install.sh --deps-only`) |

One-time on a fresh server:

```bash
# On the remote build host (from synced tree or git clone)
sudo ./install.sh --deps-only
```

## Client run

```bash
tar xzf machina-0.1.0-linux-amd64.tar.gz
cd machina-0.1.0-linux-amd64
sudo mkdir -p /etc/machina
sudo cp machina.toml.example /etc/machina/config.toml
# edit /etc/machina/config.toml (libvirt URI, TLS, etc.)
./machina-daemon --config /etc/machina/config.toml
```

Open **https://&lt;host&gt;:5092** (default port; self-signed cert unless you configure TLS).

For production, prefer **`install.sh`** or **`deploy-remote.sh`** on the hypervisor (systemd, paths under `/usr/local`).

## Client prerequisites

- **libvirt** on the host (KVM/QEMU)
- Linux **x86_64** on the **same glibc family** as the build machine (binary is not static musl)
- Optional **k3s/KubeVirt** for Kubernetes views in the UI

## vs `deploy-remote.sh`

| Script | Use when |
|--------|----------|
| **`package-binary-remote.sh`** | Tarball handoff; manual or custom install |
| **`deploy-remote.sh`** | Full rsync → build → `install.sh` → systemd on the hypervisor |

## Environment

| Variable | Default |
|----------|---------|
| `MACHINA_PACKAGE_DIR` | `~/machina-dist` |
| `MACHINA_PACKAGE_VERSION` | from `daemon/Cargo.toml` |
| `DEPLOY_SSH_TIMEOUT` | `30` (long builds use keepalive in rsync SSH) |

## Troubleshooting

| Issue | Fix |
|-------|-----|
| `make` fails on pam/libvirt | `sudo ./install.sh --deps-only` on build host |
| Wrong glibc on client | Rebuild on a host matching client OS (e.g. same Ubuntu/Debian major) |
| UI blank | Ensure `web/dist` sits next to daemon; use `install.sh` layout or set static path in config |

## Verify

```bash
curl -skS https://127.0.0.1:5092/api/v1/health
./scripts/deploy-remote.sh check USER@HOST
```
