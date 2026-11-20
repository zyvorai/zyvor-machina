---
sidebar_position: 10
title: Bare-metal provisioning
description: Take machines from a BMC address to joined hypervisor hosts, with no hand-copied media.
---

# Bare-metal provisioning

Machina can take a rack of machines that have no operating system and turn them into joined, networked hypervisor hosts.
It discovers or imports their BMCs, commissions them, keeps a pool by zone, writes a checksum-verified OS image to the
right disk, and lets each machine join the fleet on first boot. The controller does all of it; there is no cloud service
and no internet access at deploy time once images are imported.

:::caution Verification status
The whole flow has run end to end on a lab host against **virtual machines and BMC emulators** (sushy-tools for
Redfish, VirtualBMC for IPMI). It has **not** run on a physical server or a physical BMC. See
[Verification status](#verification-status) and the claims ledger (`docs/claims.md`, C49 to C51) before relying on it.
:::

## The model

A **server** is a bare-metal machine Machina knows about: a hostname, the address and credentials of its BMC, the MAC
of the NIC it network-boots from, and a zone. Two things about it are tracked separately:

- **`state`** is power: `registered`, `powered_on`, `powered_off`, `rebooting`. It comes from the BMC, or from the last
  command sent.
- **`lifecycle`** is where the machine is in provisioning.

```text
new -> commissioning -> ready -> allocated -> deploying -> deployed -> (ready | retired)
                 \-> failed -> (commissioning | new | retired)
```

Allowed moves: `new` to `commissioning`; `commissioning` to `ready`, `failed` or `new`; `ready` to `allocated`,
`commissioning` or `retired`; `allocated` to `deploying` or `ready`; `deploying` to `deployed` or `failed`; `deployed`
to `ready` or `retired`; `failed` to `commissioning`, `new` or `retired`; `retired` to `new`. Anything else is HTTP 409.
Transitions are compare-and-set, so two callers cannot both win.

## The flow

```text
 BMC --(Redfish / IPMI)-- controller --(boot override + power)--> machine
                                                                    | PXE (DHCP + TFTP)  or  BMC virtual media (iPXE ISO)
                                                                    v
                                         iPXE --- GET /boot/ipxe?mac=... ---> controller (one-time token)
                                                                    |
                    commissioning:  ramdisk --- POST report ------> controller --> lifecycle ready
                    deploy (image): ramdisk --- GET plan, image --> writes the disk, seeds cloud-init --> reboot
                                                                    | first boot: install script with a single-use token (pinned TLS)
                                                                    v
                                         POST /hosts/join --> host online, validated, site/tags/networks/pools applied
```

## What it does

### BMC control

The controller talks to the BMC itself, so these work while the machine is down.

- **Power** is idempotent and reads back the real state: `on` for a running machine and `off` for a stopped one do
  nothing, and a reset or cycle of a stopped machine powers it on. Redfish picks a reset type the BMC lists as allowed
  (many have no `PowerCycle`), follows asynchronous tasks and retries transient failures. IPMI falls back from
  `cycle` to `reset` to off and on for BMCs that reject it.
- **Boot device** override (PXE, disk, CD, once or persistent) through Redfish or IPMI. Provisioning sets it for you,
  then resets or powers on the machine, so a server with an OS on disk still reaches the boot service.
- **Virtual media**: the BMC mounts a small iPXE ISO as a virtual CD, so a machine can be provisioned with no
  DHCP/TFTP boot file on its network. BIOS and UEFI ISOs are built by `scripts/metal/build-ipxe-iso.sh`.
- **Read model** (Redfish): identity, health rollup, temperatures, power draw, firmware versions and the newest log
  entries. Sections a BMC does not report are listed, not treated as errors. A health other than `OK` raises an event.
- **Serial console** (IPMI SOL) bridged to a WebSocket, behind an audited admin token.
- **Credentials** are encrypted at rest when `MACHINA_API_KEY_MASTER_KEY` is set and are never returned by the API.

### Discovery and enrolment

A scan probes the Redfish service root on each address in a private IPv4 range and tries your credentials in order
(at most three per BMC, because BMCs lock accounts after a few failures). Authenticated BMCs can be registered in the
same call. A CSV or JSON import registers many servers and skips ones that already exist. With
`MACHINA_METAL_AUTO_ENROLL=1` (off by default) a PXE-booting machine with an unknown MAC enrols itself and is
commissioned straight away; its BMC is attached afterwards.

### Commissioning

The machine boots a small diagnostic ramdisk (kernel, initrd, squashfs; built once with mkosi), which reports CPU,
memory, disks, NICs, DMI data and four basic tests. The report is posted with a single-use token; a failed test puts the
server in `failed`, otherwise it becomes `ready`.

### Images

Images are imported once: by catalog name (Ubuntu 26.04, 24.04, 22.04, Debian 12, 13, Rocky 9, AlmaLinux 9) or by URL
with a checksum. The controller downloads them, retries dropped transfers and keeps a file only when it matches the
upstream checksum (SHA-256 or SHA-512). The deploy ramdisk verifies it again before a byte reaches the disk.

### Allocation, deploy and profiles

Allocation takes the smallest `ready` server that satisfies a request (zone, cores, memory). **Image deploy** writes the
image to the smallest suitable disk and seeds cloud-init with key-only login and a first-boot join. **Installer deploy**
renders an Ubuntu autoinstall or a RHEL-family kickstart for sites that must use the distribution installer.

A **profile** says what a machine of a kind gets (image, keys, zone, minimum size). `provision` allocates and deploys
N machines in one call; with `auto_deploy` and a desired count the controller keeps that many deployed as machines
become ready.

### Joining

A deployed machine joins over the controller's pinned TLS listener (it checks the script's checksum and the CA's
fingerprint, receives a node certificate) and the host appears with its site, rack and tags. Networks and storage pools
named on the enrolment token are set up after validation, and each outcome is written to the host's join log. Machines
that already have an OS join with `machinactl host add USER@HOST` (many at once with `--file` and `--parallel`) or, offline,
with the bundle installer's `--join`.

## Security model

- BMC and fencing passwords are encrypted at rest and reach `ipmitool` through the environment, never argv.
- Addresses, names and keys are validated before they reach a command line or a document.
- The boot, plan, report and installer endpoints have no login by design; each is gated by a single-use or short-lived
  token and rate-limited. Keep the provisioning network to the machines being provisioned. Auto-enrol widens that: any
  machine that can PXE-boot is enrolled.
- Deployed machines have no password login. Keys only, root disabled, passwordless sudo for `machina`.
- Discovery can lock BMC accounts; it is bounded to three credentials per BMC, 512 addresses and private ranges.

## Verification status

| Area | Run on the lab host (VMs and emulators) | Not run |
| --- | --- | --- |
| Redfish power, state, boot override, virtual media | sushy-tools (libvirt-backed) | physical BMCs (iDRAC, iLO, Supermicro, Lenovo, OpenBMC) |
| IPMI power, state, boot device, port, `cycle` fallback | VirtualBMC | physical BMCs |
| Redfish read model | mock server only | any emulator or hardware |
| PXE, BIOS and UEFI virtual media, commissioning, image deploy, join | VMs | physical NICs, disks and firmware |
| Image catalog | 7 images imported and verified | `ubuntu-26.04-arm64` |
| `bootstrap`, `host add --file`, labels, networks and pools, bundle `--join`, failing join | VMs | real nodes |
| Auto-enrol, `bmc-set`, profile top-up | two empty VMs | physical machines |
| Web panels | component tests and headless Chrome | a person in a headed browser |
| Serial console | pty round trip with a stand-in `ipmitool` | a real BMC's SOL |
| Installer deploys (autoinstall, kickstart) | rendering is unit-tested | no installer was run |

## Next

- [Provision your first bare-metal machines](../getting-started/provision-bare-metal.md)
- [API, CLI and configuration reference](../reference/bare-metal-api.md)
- [Troubleshooting](../operations/bare-metal-troubleshooting.md)
