# Bare Metal

## Purpose

Bare Metal — Machina Platform page at `/platform/baremetal`. It takes physical servers
from a BMC address to joined hypervisor hosts: register or discover them, commission them,
keep a pool by zone, deploy an OS image, and watch them join. Full guide:
[Bare-metal provisioning](../../../baremetal-provisioning.md).

## When to use it

- You are adding physical servers that have no OS yet, or re-imaging ones you own.
- You need to power a server through its BMC, see what its BMC reports, or open its serial console.
- Use **Hosts** (`/platform/hosts`) for machines that are already part of the fleet.

## How to get there

- Route: `/platform/baremetal`
- Nav: **Platform → Bare Metal** (or spotlight / Finder search)
- Roles: operators can look and allocate; administrators register, power, commission and deploy.

## What is on the page

1. **BMC details** (panel, opens from a server row): manufacturer, model, serial, BIOS, CPU
   and memory, health, temperatures, power draw, firmware versions and the newest BMC log
   entries. Sections a BMC does not report are listed as not reported.
2. **OS images**: pick a catalog image (Ubuntu 26.04 / 24.04 / 22.04, Debian 12 / 13,
   Rocky 9, AlmaLinux 9) and **Import**. The controller downloads it and checks it against the
   upstream checksum; the badge goes `downloading` → `ready` (or `failed` with the reason).
3. **Deploy profiles** (shown when any exist): what a machine of a kind gets (image, keys,
   zone, minimum size). Enter a number and **Provision** to allocate and deploy that many.
4. **Bare Metal Servers**: one row per server with its **lifecycle** (`new`, `commissioning`,
   `ready`, `allocated`, `deploying`, `deployed`, `retired`, `failed`), its **power state**,
   zone, and the last BMC error. Buttons: **Commission**, **Release**, **On / Off / Reset**,
   **BMC details**.
5. **Register Server**: hostname, BMC address, BMC type (Redfish, IPMI, fake), BMC
   username and password (stored encrypted, never shown again), the PXE NIC's MAC, zone.

## Operate from the console

1. Register a server (or run `machinactl metal discover --cidr … --import` for a range).
2. **Commission** it: it boots a small diagnostic image, reports its hardware and lands in
   `ready`. Failed tests put it in `failed` with the reason in the row.
3. Import an image, then create a profile (`machinactl metal profile add …`) and **Provision**.
4. Watch the lifecycle move `allocated → deploying → deployed`, and the machine appear on
   **Hosts** with its site and tags.

If the page stays empty, check the controller (`/api/v1/health`) and your role. The image and
profile panels are hidden when the controller predates the image catalog.

## Related pages

- [Bare-metal provisioning guide](../../../baremetal-provisioning.md)
- [Hosts](platform-hosts.md)
- [Security Center](../platform-security/platform-zeus-security.md)
- [Getting Started](../../getting-started.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
