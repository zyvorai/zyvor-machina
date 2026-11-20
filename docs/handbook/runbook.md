# Machina — Operator runbook

Day-to-day procedures for a single host and for a controller-managed fleet. Symptom-by-symptom fixes are in
[troubleshooting.md](troubleshooting.md); configuration is in [admin-configuration.md](admin-configuration.md).

## Install and upgrade

```bash
# From a source tree on the host
sudo ./machinactl deploy                       # deps, build, install, start, verify
sudo ./install.sh --bind 0.0.0.0 --open-firewall

# From a customer tarball
tar xzf machina-*-linux-amd64.tar.gz && cd machina-*-linux-amd64
sudo ./install-everything.sh

# From a laptop
./scripts/deploy-remote.sh USER@HOST --quick [--platform]
```

Upgrade in place with `sudo ./machinactl upgrade` (git pull, rebuild, reinstall, verify). For a fleet: back up the
controller database, upgrade the controller, then hypervisors one at a time through maintenance mode. Upgrade
`machina-bpfd` and `machina-cni` together. See the website's
[upgrade and backup guide](https://zyvorai.github.io/machina/docs/operations/upgrade-backup).

Package for customers:

```bash
./scripts/package-binary-remote.sh BUILD_HOST USER --from-deploy --fetch
```

Hand off `dist/machina-*-linux-amd64.tar.gz` and its `.sha256`.

## Health checks

```bash
./machinactl health                              # exit 0 healthy, 1 degraded, 2 critical
curl -sk https://127.0.0.1:5092/api/v1/health
sudo systemctl status machina-daemon libvirtd machina-bpfd
sudo journalctl -u machina-daemon -f
```

## Backup and restore

- VMs: **Backups** in the UI, `sudo ./machinactl backup now`, or `POST /api/v1/backups`. Verify with
  `POST /api/v1/backups/{id}/verify`. Nightly: `sudo ./machinactl backup enable`.
- Restore: follow the README in `/var/lib/machina/backups/<timestamp>/`, or restore from VM detail.
- Controller database (either backend): `sudo machinactl db backup` (an online SQLite copy, or a `pg_dump` for PostgreSQL; the PostgreSQL
  setups also install a daily timer, dumps in `/var/backups/machina/db`), plus `/etc/default/machina-platform`. Restore a PostgreSQL
  dump with `sudo machinactl db restore FILE --yes`; for SQLite stop the controller and copy the file back. See
  [the database guide](../guides/database.md).

## Sign-in

- Entry: `https://HOST:5092/` or `/login`. After sign-in the URL should be `/platform`.
- A 404 with breadcrumb `login` after PAM login means a stale web bundle: redeploy with `--quick`.
- Verify a PAM session from the host:

```bash
curl -sk -c /tmp/machina.jar -X POST https://127.0.0.1:5092/api/v1/auth/login \
  -H 'Content-Type: application/json' -d '{"username":"USER","password":"PASS"}'
curl -sk -b /tmp/machina.jar https://127.0.0.1:5092/api/v1/auth/session | jq
```

- Logs: `journalctl -u machina-daemon -f`, look for `PAM login successful`.
- LDAP: see [../ldap-auth.md](../ldap-auth.md); map AD groups with `admin_group_substrings` /
  `operator_group_substrings`.

## Remote access not working

1. `ss -tlnp | grep 5092` must show `0.0.0.0:5092`.
2. `grep '^host' /etc/machina/config.toml` should be `0.0.0.0` for LAN/WAN access.
3. Firewall: `firewall-cmd --list-ports` or `iptables -L INPUT -n | grep 5092`.
4. Re-run `sudo ./install.sh --bind 0.0.0.0 --open-firewall`.

## Fleet (controller)

### Host down / agent disconnected

1. Platform → Hosts: check state and `last_heartbeat_at`. A host goes offline after 90 s without a heartbeat.
2. On the hypervisor: `systemctl status machina-agent libvirtd`.
3. Check the gRPC port (50051) and the firewall between controller and host.
4. Re-run validation: **Validate** in the UI or `POST /api/v1/hosts/{id}/validate`.
5. After the agent recovers, **Sync** the host inventory.

### HA restart failed

1. Platform → HA: check the HA events and fence events.
2. Recovery is blocked until the failed host is fenced. Verify IPMI credentials on the host record, or
   `MACHINA_FENCE_COMMAND` on the agent.
3. Confirm a destination has capacity (placement recommendations). See [../controller-ha.md](../controller-ha.md).

### Migration failed

1. Open the `vm.migrate` task for the failure message.
2. Pre-check: `POST /api/v1/vms/{id}/migrate/precheck` with the destination host.
3. Fix failing checks (memory headroom, CPU compatibility, maintenance mode, libvirt URI, ports 49152–49215/tcp).
4. Retry; use `live: false` when CPU compatibility blocks live migration.

### Host join validation failed

1. Review `validation_report` on the host detail page.
2. Common fixes: start libvirtd, install qemu-kvm, open 49152–49215/tcp between nodes.
3. Re-run `POST /api/v1/hosts/{id}/validate`.

### Bare-metal provisioning stuck

Guide and full troubleshooting table: [baremetal-provisioning.md](../baremetal-provisioning.md).

1. `machinactl metal list`: look at `LIFECYCLE`, `POWER` and `ERROR` (`last_error`) for the server.
2. **Stays `commissioning` or `deploying`:** the machine did not reach the boot service.
   Check its MAC is registered (`mac_address`), that it booted from the network or the virtual
   CD (`machinactl metal power ID cycle`; `POST …/boot-device` `pxe`), and that the BMC is
   reachable (`GET …/power-state`).
3. **`failed`:** `last_error` says why (a commissioning test, a checksum mismatch, not
   enough RAM to stage the image). Fix it, then `machinactl metal commission ID` again.
4. **Deployed but never joins:** on the node `cloud-init status` and `journalctl -u machina-agent`;
   the controller's TLS join listener must be reachable. The host's join log lists each step.
5. **Release a machine:** `machinactl metal release ID` (allocated or deployed → ready).

### Rolling agent upgrade

1. Put the host in maintenance (optionally evacuate).
2. `POST /api/v1/hosts/{id}/upgrade` with a target from `GET /api/v1/upgrade/matrix`.
3. Restart `machina-agent`; confirm validation passes.

### Support bundle

```bash
./scripts/platformctl support-bundle > bundle.json
```

Includes task failures, the audit tail and the host version matrix.

## Native eBPF

All procedures below are safe by default: enforcement is lease-gated, fails open and is never persisted. Practise on
a lab host first.

### Arm enforcement for a maintenance window

1. Create or review policies (Platform → Security, or `POST /api/v1/bpf/policies`) and watch the `observed` counters
   in observe mode first.
2. Native eBPF → Overview → set the lease (minutes) → **Enforce**. Or `PUT /api/v1/bpf/mode`.
3. When the window ends the lease lapses by itself; **Observe** ends it early.

### Emergency: isolate a compromised host

1. Native eBPF → **Node Isolation**. Keep **dry run** on and check what would be dropped.
2. Confirm SSH (22) is allowlisted or add your admin CIDR to exempt — arming is refused otherwise.
3. Arm with the shortest useful lease (10–900 s); extend by re-arming. It reverts on its own at the deadline.

### DDoS on the uplink

1. Native eBPF → **Shield**, mode **audit**, and look at the top over-rate sources.
2. Tune per-class rates and allow/deny CIDRs, then switch to **enforce** under the enforcement lease.

### bpfd restarted

Everything comes back in observe mode. Re-arm any enforcement you still need. `journalctl -u machina-bpfd` shows
load errors; `GET /api/v1/bpf/status` shows `programs_compiled` and kernel `features`.

## Compliance

See [../compliance-hardening.md](../compliance-hardening.md).
