---
slug: fluxvm-day-two
title: FluxVM day two — the same VM page as libvirt
authors: [zyvor]
tags: [fluxvm, release, consoles]
description: "v0.3.2 closes FluxVM day-two gaps — browser VNC, data disks, clone, Secure Boot/TPM, snapshots and backups through the Machina UI you already use."
---

Machina already listed FluxVM guests next to libvirt ones. What operators actually needed was the rest of the day: a
screen during install, another disk, a clone before a risky change, a snapshot and a backup that the fleet can drive.

**v0.3.2** ships that for FluxVM QEMU.

{/* truncate */}

## What you can do now

- **Browser VNC** through Machina (`ws-ticket` into fluxvm-api — no host `socat`).
- **Data disks** attach, grow and detach from **Manage**.
- **Clone / rename**, console **logs**, **balloon**, **Secure Boot / TPM** on create.
- **Snapshots and backups** on the daemon and, with the agent pointed at FluxVM, on the controller too.

The VM list does not care which backend started the guest. Open it the same way.

## Try it in twenty minutes

Follow [FluxVM day two](/docs/getting-started/fluxvm-day-two): enable `[fluxvm]`, create a small QEMU guest, open VNC,
attach a disk, snapshot, backup and clone. The long form lives in the repo as
[tutorial 07](https://github.com/zyvorai/zyvor-machina/blob/main/docs/tutorials/07-fluxvm-day-two.md).

## What we ran

On a lab host with FluxVM 0.4.18, `make regression-fluxvm` for QEMU + shared-disk migrate/HA was **35/35**. Firecracker
start-after-restore still fails on that FluxVM build (host-side); we call it out in the [changelog](https://github.com/zyvorai/zyvor-machina/blob/main/CHANGELOG.md)
and [claims](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md).

## Read more

- [FluxVM backend](/docs/core-concepts/fluxvm)
- [Operator guide](https://github.com/zyvorai/zyvor-machina/blob/main/docs/fluxvm.md)
- [Release v0.3.2](https://github.com/zyvorai/zyvor-machina/releases/tag/v0.3.2)
