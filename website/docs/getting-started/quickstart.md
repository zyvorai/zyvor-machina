---
sidebar_position: 1
title: Quickstart
description: Install Machina on a Linux KVM host with one command and sign in.
---

# Quickstart

Machina installs on any Linux host with KVM: Ubuntu, Debian, Fedora, RHEL/Alma/Rocky, openSUSE or Arch.

## One host

```bash
git clone https://github.com/zyvorai/zyvor-machina.git machina && cd machina
./machinactl deploy        # deps · build · install · start · verify
```

`machinactl deploy` installs dependencies (libvirt, QEMU, build tools), builds a release, runs `make install`,
starts the `machina-daemon` service and smoke-tests the API on `https://127.0.0.1:5092`.

Open `https://<host>:5092` and sign in with a local Linux account. Authentication goes through PAM, so any user who
can log in to the host can log in to Machina, subject to role-based access.

```bash
./machinactl status        # systemctl status machina-daemon
./machinactl health        # deep health check: exit 0 healthy, 1 degraded, 2 critical
```

## From your laptop to a remote host

`deploy-remote.sh` rsyncs the sources to the server and builds there. Nothing compiles locally, so you only need
`ssh` and `rsync`.

```bash
./scripts/deploy-remote.sh USER@HOST                  # daemon + web UI
./scripts/deploy-remote.sh USER@HOST --platform       # plus controller and agent
./scripts/deploy-remote.sh USER@HOST --open-firewall  # also open 5092 in the host firewall
```

The installer generates a self-signed certificate; replace it with one from your CA for production browsers.

## Grow into a fleet

Add `--platform` to also install `machina-controller` (`:5093`) and `machina-agent` (`:50051`). The controller turns
a set of hosts into one pool with HA failover, DRS, live migration and the self-service Fleet Cloud. See
[Architecture](../core-concepts/architecture.md) and [Controller HA](../core-concepts/controller-ha.md).

## Turn on the eBPF datapath

`machina-bpfd` is built and installed with the rest of Machina when the eBPF toolchain is present (`make bpf-deps`).
Open **Platform → Security → Native eBPF** to see what your kernel supports and what the datapath observes. See
[Native eBPF](../networking/ebpf-overview.md).

## Next steps

| Next step | Where |
| --- | --- |
| Hardware and OS requirements | [Requirements](requirements.md) |
| Ports, auth, TLS, config | [Configuration](../operations/configuration.md) |
| Browser consoles | [Consoles](../core-concepts/consoles.md) |
| Create and manage VMs | [Virtual machines](../core-concepts/virtual-machines.md) |
| Something not working | [Troubleshooting](../operations/troubleshooting.md) |
| Production pilot checklist | [Customer site readiness](https://github.com/zyvorai/zyvor-machina/blob/main/docs/CUSTOMER_SITE_READINESS.md) |
