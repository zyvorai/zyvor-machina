# Machina Platform Certification Matrix

Initial certification targets for batch 16. Expand with customer-driven OS/storage combinations.

## Hypervisor OS

| OS | libvirt | QEMU/KVM | Status |
|----|---------|----------|--------|
| RHEL 9 / Rocky 9 | 9.x | 8.x | Primary E2E target |
| Ubuntu 22.04 LTS | 8.x | 6.x | Supported |
| Ubuntu 24.04 LTS | 10.x | 8.x | Supported |
| Debian 12 | 9.x | 7.x | Best-effort |

## Storage backends (batch 14)

| Backend | Agent provision | Notes |
|---------|-----------------|-------|
| directory | `virsh pool-define-as … dir` | Default VM disks |
| lvm / lvm-thin | `logical` pool type | Requires LVM setup on host |
| nfs | `netfs` pool type | Mount path must exist |

## Network backends (batch 14)

| Type | Agent provision |
|------|-----------------|
| linux-bridge | `virsh net-define` + bridge |
| VLAN | libvirt VLAN tag on bridge |

## Migration

| Mode | Requirement |
|------|-------------|
| Live | Shared storage or block migration; VM running; CPU compat |
| Offline | VM stopped or shutoff |

## Controller

| Deploy | Notes |
|--------|-------|
| Single node | Default install.sh |
| Multi-controller | DB-lease leader election over a shared SQLite store + NATS fan-out (see [controller-ha.md](controller-ha.md#multiple-controllers)) |

Run full suite: `./scripts/e2e-platform-test-remote.sh user host`

Chaos smoke: `./scripts/e2e-chaos-platform.sh user host`
