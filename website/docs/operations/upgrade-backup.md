---
sidebar_position: 4
title: Upgrade and backup
description: Upgrade Machina in place, back up VMs and controller state, and roll back.
---

# Upgrade and backup

## Upgrade

On a host installed from a git checkout:

```bash
sudo ./machinactl upgrade     # git pull --ff-only, rebuild, reinstall, restart, verify
```

From your laptop:

```bash
./scripts/deploy-remote.sh USER@HOST --quick              # incremental build + restart
./scripts/deploy-remote.sh USER@HOST --quick --platform   # include controller and agent
```

Upgrade order for a fleet:

1. Back up the controller database (below).
2. Upgrade the controller host.
3. Upgrade hypervisors one at a time: put the host in maintenance (DRS evacuates it), upgrade, take it out of
   maintenance.
4. Upgrade `machina-bpfd` and `machina-cni` together on each host; bpfd rejects a CNI sync from a different ABI
   version.

`machina-bpfd` restarts in observe mode by design: enforcement leases are never persisted, so re-arm any enforcement
you need after the upgrade.

## Back up VMs

```bash
sudo ./machinactl backup now       # back up VM definitions (and disks if configured) now
sudo ./machinactl backup enable    # nightly at 02:00 via machina-backup.timer
./machinactl backup status         # timer state, count and size
```

Backups land in `/var/lib/machina/backups` (`[backup] backup_dir`). Set `with_disks = true` to include disk images,
`nfs_target` to copy them off-host and `retain` (default 7) to keep that many generations. From the UI, back up and
restore individual VMs from VM detail.

## Back up the controller

All controller state is one SQLite file, `/var/lib/machina/controller.db`. Back it up with SQLite's online backup so
the copy is consistent while the controller runs:

```bash
sudo sqlite3 /var/lib/machina/controller.db ".backup '/var/backups/controller-$(date +%F).db'"
```

Also keep `/etc/default/machina-platform` (secrets such as `MACHINA_JWT_SECRET` and `MACHINA_API_KEY_MASTER_KEY`).
Without the master key, stored LLM provider keys cannot be decrypted.

## Back up the daemon

Keep `/etc/machina/` (config and TLS certificate), `/etc/default/machina-daemon`, and `/var/lib/machina/` (roles,
audit log, metrics history, backups).

## Roll back

Reinstall the previous release (`git checkout <tag> && sudo ./machinactl reinstall`) and, for the controller, stop it
and restore the database copy before starting it again.
