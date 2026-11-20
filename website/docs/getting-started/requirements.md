---
sidebar_position: 2
title: Requirements
description: Host, OS and network requirements for Machina.
---

# Requirements

## Host

- A 64-bit Linux host with hardware virtualization (Intel VT-x or AMD-V) enabled, so `/dev/kvm` exists.
- x86_64 or aarch64. The installer picks the matching QEMU system emulator (`qemu-system-x86` or `qemu-system-arm`)
  plus `qemu-utils`. Release packages are published for both `amd64` and `arm64`; the arm64 packages are built on
  native runners and have not yet been run on a production arm64 host.
- libvirt (`libvirtd`). The daemon talks to `qemu:///system` by default.
- Enough disk for guest images under the libvirt storage pools you configure.

## Kernel (native eBPF)

The VM and fleet features run on any supported distribution kernel. The eBPF datapath (`machina-bpfd`) uses more of
the kernel; each feature turns itself off when its requirement is missing, and `GET /api/v1/bpf/status` shows what is
available.

| Requirement | Needed for |
| --- | --- |
| BTF (`/sys/kernel/btf/vmlinux`) and cgroup v2 | Most programs |
| Kernel 6.6+ (TCX) | VM edge isolation, node isolation |
| BPF-LSM active (`lsm=...,bpf`) | VMM guard enforcement (audit works without it) |
| Kernel 6.12+ with sched_ext | The optional VM scheduler |
| `CONFIG_XDP_SOCKETS` | AF_XDP |

Building the eBPF programs needs a nightly Rust toolchain and `bpf-linker`; `make bpf-deps` installs both.

## Supported distributions

Ubuntu, Debian, Fedora, RHEL / AlmaLinux / Rocky Linux, openSUSE and Arch Linux. The live screenshots on this site
come from Ubuntu 26.04.

## Build

The Rust workspace depends on Linux libvirt headers, so it builds on Linux only. From macOS or Windows, use
`./scripts/deploy-remote.sh`, which builds on the target host.

## Network ports

| Port | Component | Default bind |
| --- | --- | --- |
| `5092` | `machina-daemon` (HTTPS, web UI + API) | `0.0.0.0` |
| `5093` | `machina-controller` | `0.0.0.0` |
| `50051` | `machina-agent` gRPC | `127.0.0.1` |
| `50052` | `machina-agent` console proxy | `127.0.0.1` |

`machina-bpfd` listens only on a local Unix socket. Full list: [Ports and sockets](../reference/ports.md).

Use `--open-firewall` with `deploy-remote.sh` or `install-platform.sh` to open the firewall for you.

## Browser

Any current Chrome, Edge, Firefox or Safari. Consoles run in the browser (noVNC, SPICE, serial, SSH), so operators
need nothing else installed.
