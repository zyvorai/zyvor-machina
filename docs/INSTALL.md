# Installing Machina

Three ways in, all from the same release assets. Pick by how much network the host has.

| You have | Use |
|----------|-----|
| a package repo / the `.deb` or `.rpm` files | **Packages** — `apt`/`dnf` handle dependencies and upgrades |
| an air-gapped or locked-down host | **Offline bundle** — one tarball, one script, no network |
| the source tree | `./machinactl deploy` (builds from source; see the README) |

Every release ships `SHA256SUMS` and a CycloneDX SBOM (`machina-<version>.sbom.cdx.json`). Verify before installing:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

## What gets installed

| Package | Contains | Put it on |
|---------|----------|-----------|
| `machina` | daemon, web UI, `machinactl`, backup timer | the host you browse to (and every host if you want per-host consoles) |
| `machina-controller` | fleet control plane (127.0.0.1:5093 behind the daemon's proxy) | one management host |
| `machina-agent` | agent + `machina-bpfd` (eBPF datapath) | every compute node |

A **single host** needs all three. A **fleet** is one controller plus an agent on each compute node.

## Single host, packages (≈ 5 minutes)

```bash
# Debian / Ubuntu
sudo apt install ./machina_*_amd64.deb ./machina-controller_*_amd64.deb ./machina-agent_*_amd64.deb
# RHEL / Rocky / Alma / Fedora
sudo dnf install ./machina-*.rpm
```

The first install generates `/etc/default/machina-platform` (controller JWT secret, controller↔agent token, bootstrap admin
password; mode 0600) and starts the services. Then:

```bash
sudo cat /etc/machina/INITIAL_ADMIN_PASSWORD     # sign in as admin; change it after first login
```

Open `https://<host>:5092`. The certificate is self-signed until you install your own (see `docs/handbook/admin-configuration.md`, TLS).
Set `MACHINA_NO_START=1` before installing to skip starting the services.

## Air-gapped / offline bundle

Copy `machina-<version>-linux-amd64.tar.gz` to the host (USB, internal mirror), then:

```bash
tar xzf machina-*-linux-amd64.tar.gz && cd machina-*-linux-amd64
sudo ./install.sh --all            # or --daemon / --controller / --agent
```

The script verifies the bundle, checks that libvirt and QEMU are present (it **never** installs OS packages and says exactly
what is missing), installs to `/usr/local`, creates first-run secrets, and starts the services. `--bind 0.0.0.0` makes the
web UI listen on all interfaces. `--uninstall [--purge]` removes it (data is kept unless you pass `--purge`).

## Before you install, and after

`machina-preflight` (also `machinactl preflight`) answers "can this machine run Machina?": hardware virtualisation, `/dev/kvm`, libvirt and QEMU, kernel BTF and cgroup v2, free ports, disk, memory, clock and (with `--controller URL`) reachability of the controller. Every failed line says what to do; `--role agent|controller|daemon` checks only what that role needs, `--json` is for scripts, and `--fix` installs missing libvirt/QEMU with your package manager after asking.

```bash
sudo machina-preflight --role agent --controller https://<controller-host>:5093
```

After installing, `sudo machinactl show-login` prints the URL, the user name and the generated admin password (also kept in `/etc/machina/INITIAL_ADMIN_PASSWORD`, root only). Change the password after your first sign-in.

## Adding a compute node to a fleet

The short path is in [QUICKSTART.md](QUICKSTART.md): turn on the controller's join listener (`MACHINA_CONTROLLER_TLS_ADDR`), run `machinactl dist publish`, then add a node from the web (Platform → Hosts → Add machine), with `machinactl host add user@node`, or by pasting the one command the wizard (`/platform/enroll`) shows. The manual steps below do the same thing in pieces.

On the controller create a one-time enrollment token (`POST /api/v1/enrollment/tokens`, admin only), then on the new host:

```bash
sudo apt install ./machina-agent_*_amd64.deb         # or: sudo ./install.sh --agent
sudo machina-agent join --controller https://<controller-host>:5093 --token <enrollment-token>
```

`join` also receives an agent token that belongs to this host alone (the controller generates it, keeps it with the host record and presents it only to this host's agent). The agent stores it in `/etc/default/machina-platform` (mode 0600, the old file kept as `.bak-pre-join`) and restarts `machina-agent`, so no secret is copied by hand and no fleet-wide secret is ever sent. It only stores the token when the controller URL is `https://` or loopback (an SSH tunnel); otherwise it says so and leaves the file alone. The agent stays on loopback unless you add `--expose` (for a controller on another machine): it then listens on this host's own address, never `0.0.0.0` (`MACHINA_AGENT_LISTEN` / `MACHINA_AGENT_CONSOLE_LISTEN` in the same file), only together with its own token. Traffic is token-protected, not encrypted, unless `MACHINA_AGENT_TLS_CERT`/`KEY` are set, so allow only the controller to reach ports 50051-50052. Hosts that did not join this way keep using the shared `MACHINA_AGENT_TOKEN`. Re-joining a host issues it a new token.

Tokens are single-use and can expire; the host appears as *pending validation* until the controller has reached its agent.

### Many nodes, labels, and machines with no OS yet

- `sudo machinactl bootstrap --public-url https://<controller>:5093 --tls-addr 0.0.0.0:5094` sets
  `MACHINA_PUBLIC_URL` and the join listener in `/etc/default/machina-platform` (mode 0600),
  restarts the controller if they changed, publishes the agent and waits for health. Run it once.
- `machinactl host add --file hosts.txt --parallel 8 --site fra1 --rack r12 --tag gpu --network br-prod --pool ssd-pool`
  joins many nodes over SSH, one single-use token each. `site`, `rack`, `tags` are set when
  the host joins; the named, existing networks and storage pools are set up once it passes
  validation (each outcome is in the host's join log). The same fields exist on
  `POST /api/v1/enrollment/tokens` (`cluster_id`, `site`, `rack`, `tags`, `networks`, `storage_pools`).
- Offline: `sudo ./install.sh --agent --join https://<controller>:5094 --token-file /root/join.token --ca-sha256 <fp>`
  installs from the bundle and joins in one step (libvirt and QEMU must already be there).
- Bare machines with no OS: [baremetal-provisioning.md](baremetal-provisioning.md) takes them
  from a BMC address to a joined host (discover, commission, deploy an image, join).

## Compatibility

The release is built on **Ubuntu 22.04** (glibc 2.35, libvirt 8.0), so the binaries need **glibc 2.34 or newer** and
libvirt 8.0 or newer — verified by building there and starting `machina-daemon`, `machina-controller` and `machina-agent`
on a clean 22.04. Distributions that ship at least that (Ubuntu 22.04+, Debian 12+, RHEL/Rocky/Alma 9+) are expected to
work; only Ubuntu 22.04 and 26.04 and Fedora have been exercised so far. If a binary refuses to start with a `GLIBC_` or
`libvirt` error, use assets built for your distribution. Needs x86_64, KVM (`/dev/kvm`) and QEMU.

## Upgrading

Packages: `apt install ./new.deb` / `dnf upgrade ./new.rpm`. Configuration (`/etc/machina/config.toml`,
`/etc/default/machina-*`) and data (`/var/lib/machina`) are kept; services restart on upgrade. Offline bundle: run the new
`install.sh` again. Upgrade the controller before the agents.

Release directory or source checkout (controller, daemon and the agent on this machine) in one safe step:

```bash
sudo machinactl upgrade --from DIR --dry-run   # what would change, version-skew check, nothing touched
sudo machinactl upgrade --from DIR             # DIR holds machina-controller, machina-daemon, machina-agent, machina-bpfd (+ web/, VERSION)
sudo machinactl upgrade --pull                 # in a git checkout: git pull, cargo build --release, then upgrade
```

It takes `machinactl backup all` first (nothing is changed if the backup fails), then upgrades the **controller, then the
daemon, then the agent**. Each step swaps the binary (the old one stays as `<binary>.prev`), restarts its service and waits
(`--health-wait`, default 60 s) for it to be healthy: the controller must answer `/api/v1/health` with the database ok, the
daemon its health endpoint, the agent must stay active. A step that is not healthy is **rolled back at once**: its previous
binary returns, and a failed controller also gets the pre-upgrade database back from the backup (the new version may have
migrated it), then the command exits 1 and nothing after that step is touched. It refuses to move to an older version
(`--allow-downgrade` overrides) and refuses when a remote agent is newer than the new controller. Remote agents are listed
(`machinactl host list` has an AGENT column) but not touched: upgrade them after the controller, with their package, or by
re-running the install command from the controller (the new agent is published to `/dist` for joining nodes).
`--no-backup` skips the backup (a failed controller then only gets its old binary back), `--no-agent` leaves the agent alone.

## Backup and restore the controller

`sudo machinactl backup all` writes one archive (`/var/backups/machina/full/machina-full-<time>.tar.gz`, mode 0600, newest 7 kept; `--out DIR`, `--keep N`) holding everything a lost controller needs: the database (an online SQLite copy, or a PostgreSQL dump), `/etc/default/machina-*` (JWT and agent secrets), `/etc/machina` (config, TLS, the first-run password), the fleet and VM-network-policy CAs and published agent files, with a checksum manifest. The archive contains secrets: keep it private.

`sudo machinactl restore FILE` verifies the checksums, refuses a damaged archive, keeps the current files under `/var/lib/machina/pre-restore-<time>/`, restores, restarts the controller and daemon and checks health (`--yes` skips the question). Take a backup before every upgrade.

## Uninstall

`apt remove machina machina-controller machina-agent` (or `dnf remove …`) keeps your data; `apt purge` / deleting
`/etc/machina`, `/etc/default/machina-*` and `/var/lib/machina` removes it.
