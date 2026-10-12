---
sidebar_position: 5
title: Troubleshooting
description: The first checks to run, and fixes for the most common problems.
---

# Troubleshooting

## First checks

| Goal | Command |
| --- | --- |
| Host readiness | `./machinactl doctor` |
| Deep health check (exit 0 / 1 / 2) | `./machinactl health` |
| Service state | `systemctl status machina-daemon machina-controller machina-agent machina-bpfd` |
| Live logs | `journalctl -u machina-daemon -f` |
| API health | `curl -sk https://127.0.0.1:5092/api/v1/health` |
| libvirt reachable | `virsh -c qemu:///system list --all` |
| eBPF datapath | `curl -sk -b cookies https://127.0.0.1:5092/api/v1/bpf/status` |

## Install and access

- **Can't reach `:5092` from another machine**: open the port (`--open-firewall` on deploy) and check `[daemon] host`.
- **Browser TLS warning**: the installer's certificate is self-signed; install one from your CA in `/etc/machina/ssl/`.
- **Login fails with correct Linux credentials**: sign-in uses the PAM service in `[auth] pam_service` (default
  `sshd`); check `journalctl -u machina-daemon` for the PAM error.
- **Platform pages fail**: the controller isn't running or wasn't installed; deploy with `--platform` and check
  `systemctl status machina-controller`.

## VMs

- **No `/dev/kvm`**: enable VT-x/AMD-V in firmware (or nested virtualization on a cloud VM).
- **Create fails**: the error banner names the failing step; most often a missing storage pool, a full disk or a
  network that isn't started.
- **Live migration stalls**: busy guests dirty memory faster than it copies; raise the downtime tolerance or migrate
  in a quieter window.

## Native eBPF

| Symptom | Cause and fix |
| --- | --- |
| `programs_compiled: false` | Built without the eBPF toolchain. Run `make bpf-deps`, rebuild and reinstall. |
| Features missing in `/bpf/status` | The kernel lacks them: TCX needs 6.6+, sched_ext 6.12+, most programs need BTF (`/sys/kernel/btf/vmlinux`). |
| VMM guard reports `lsm_inactive` | BPF-LSM is not active. Add `bpf` to the kernel's `lsm=` boot parameter and reboot; until then the guard stays in audit. |
| Enforcement stopped by itself | Its lease expired or bpfd restarted. This is intentional; re-arm with a new lease. |
| Node isolation refuses to enable | Allowlist TCP 22 or add an exempt CIDR first. |
| CNI sync rejected | `machina-bpfd` and `machina-cni` are different versions; upgrade both. |
| `machina-cni` exits with status 78 | Another CNI is already configured; machina-cni is opt-in and will not replace it. Keep that CNI, or set `MACHINA_CNI_TAKEOVER=1` in `/etc/default/machina-cni` to replace it. |

## More

The full symptom-by-symptom guide is the
[troubleshooting handbook](https://github.com/zyvorai/zyvor-machina/blob/main/docs/handbook/troubleshooting.md).

Bare-metal provisioning problems (BMC power, PXE and virtual media, commissioning, image deploy, join) have their own
page: [Bare-metal troubleshooting](bare-metal-troubleshooting.md).
