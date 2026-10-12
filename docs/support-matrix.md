# Support matrix

What Machina has been built and exercised on, and what it has not. "Expected to work"
means the code paths exist but nobody has run them; it is not a support commitment.
Sources: [INSTALL.md](INSTALL.md), `install.sh`, `packaging/nfpm/`,
`.github/workflows/release.yml`, [claims.md](claims.md).

## Host operating systems

| OS | Status |
|---|---|
| Ubuntu 22.04, 26.04 | Built and exercised (release CI verifies the `.deb` on 22.04) |
| Fedora | Exercised (release CI verifies the `.rpm` on `fedora:latest`) |
| Debian 12+, RHEL / Rocky / Alma 9+ | Expected to work; `install.sh` has the package-manager paths; no CI gate |
| openSUSE / SLES, Arch | `install.sh` has `zypper` / `pacman` branches; not exercised |

## CPU architecture

| Arch | Status |
|---|---|
| x86_64 | The only released architecture. Packages and the offline bundle are `amd64` |
| aarch64 | `install.sh` accepts it and has aarch64 branches (Cloud Hypervisor, Firecracker, bpf-linker, QEMU), but no aarch64 package, no CI runner and no test run. Treat as unsupported |

Hardware needs KVM. Preflight (`machinactl preflight`) checks KVM, BTF, cgroup v2,
ports, disk, memory and the clock.

## Bare-metal management (BMC) and provisioning

Guide: [baremetal-provisioning.md](baremetal-provisioning.md). Status per claim:
[claims.md](claims.md) C49 to C51. "Emulator" below means a lab host with sushy-tools
(Redfish, libvirt-backed) or VirtualBMC (IPMI, libvirt-backed) driving throwaway VMs.

| Interface | Capabilities | Verified on |
|---|---|---|
| Redfish | power on/off/cycle/reset (idempotent, `ResetType` from the BMC's allowed list, async tasks, retries), power-state read-back, boot-device override, virtual media (InsertMedia/EjectMedia), read model (identity, health, temperatures, power, firmware, log) | power, state, override, virtual media: **emulator**; read model: mock server only |
| IPMI (`ipmitool` lanplus) | power, power-state, boot device, `host:port`, `cycle` fallback, serial-over-LAN console | power, state, boot device, port, cycle fallback: **emulator** (VirtualBMC); SOL: pty round trip with a stand-in `ipmitool` |
| libvirt (lab only) | power, power-state for `metal-test-*` domains | lab host |
| Vendors (Dell iDRAC, HPE iLO, Supermicro, Lenovo XCC, OpenBMC) | none validated on hardware | record the models you test in `claims.md` C51 |

| Provisioning step | Verified on |
|---|---|
| PXE (proxyDHCP/TFTP config) and iPXE chain | VM: the controller-rendered iPXE script; the dnsmasq config is unit-tested only |
| Virtual media boot (BIOS iPXE ISO) | sushy-tools + VM; the UEFI ISO builds but has not booted |
| Commissioning ramdisk, inventory, `ready` | VM |
| Image deploy (Ubuntu 26.04), cloud-init seed, first-boot join over pinned TLS | VM |
| OS image catalog (Ubuntu 26.04/24.04/22.04, Debian 12/13, Rocky 9, AlmaLinux 9) | imported and checksum-verified on the lab host; `ubuntu-26.04-arm64` not imported |
| `bootstrap`, `host add --file --parallel`, join labels, post-join networks and pools | two VMs and one VM |
| Installer deploy (autoinstall, kickstart) | rendering is unit-tested; no installer was run |

What the controller host needs: `ipmitool` (IPMI power, SOL), util-linux `script` (SOL),
`virsh` (libvirt driver, labs only). Ramdisk and ISO builds need a Linux build host with
mkosi (ramdisk) or git/make/gcc/xorriso (iPXE ISOs).

## Database

| Backend | Status |
|---|---|
| SQLite (embedded, default) | What every deployment so far runs |
| PostgreSQL 16 | Schema loads and every literal SQL statement passes `check_pg_sql.py`; no live fleet has run on it ([controller-ha.md](controller-ha.md)) |

## Browsers and languages

The UI is English with partial Spanish (`en`, `es`). Dates and numbers go through
`web/src/utils/localeFormat.ts`; most pages still call the browser's `toLocale*`
directly. Right-to-left layout is not implemented (the document `dir` is set, the CSS
is not mirrored).

## Not documented anywhere yet

Hosts per controller, controller sizing, RPO/RTO for a full restore, cross-site live
migration limits and WAN tuning, and a FIPS-validated crypto path. These need
measurements; none have been taken, so nothing is claimed here.
