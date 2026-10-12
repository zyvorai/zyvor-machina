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

## Bare-metal management (BMC)

| Interface | Status |
|---|---|
| IPMI (`ipmitool`, lanplus) | Fencing and power code exist; **never run against real hardware** |
| Redfish | Power on/off/cycle/reset implemented; unit-tested only; no vendor validated |
| Vendors (Dell iDRAC, HPE iLO, Supermicro, Lenovo XCC) | None validated. Record the models you test in `claims.md` C49 |

Network boot (PXE/iPXE) and OS deploy are described in
[baremetal-provisioning.md](baremetal-provisioning.md); none of it has run on real
hardware yet.

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
