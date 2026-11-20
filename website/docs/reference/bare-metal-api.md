---
sidebar_position: 4
title: Bare-metal API and CLI
description: Endpoints, machinactl commands and configuration for bare-metal provisioning.
---

# Bare-metal API and CLI

All endpoints are under `/api/v1/baremetal` on the controller (through the daemon at
`/api/v1/platform/controller`). Roles are Machina roles; "token" means a single-use or short-lived token instead of a
sign-in.

## Endpoints

| Method and path | Role | Purpose |
| --- | --- | --- |
| `GET /servers`, `POST /servers` | operator, admin | List; register |
| `PUT /servers/{id}/bmc` | admin | Attach or change BMC type, address and login (audited) |
| `POST /import` | admin | Register up to 500 servers; existing ones are skipped |
| `POST /discover` | admin | Scan a private IPv4 range for Redfish BMCs, optionally register them |
| `POST /servers/{id}/power` | admin | `on`, `off`, `cycle`, `reset` (`dry_run`) |
| `GET /servers/{id}/power-state` | operator | What the BMC reports |
| `POST /servers/{id}/boot-device` | admin | `pxe`, `disk`, `cdrom`, once or persistent |
| `POST /servers/{id}/virtual-media` | admin | `insert` or `eject` |
| `POST /servers/{id}/bmc-info/refresh`, `GET /servers/{id}/bmc-info` | operator | Redfish read model |
| `POST /servers/{id}/sol-token` | admin | Serial console token (audited) |
| `POST /servers/{id}/commission` | admin | Start commissioning |
| `GET /servers/{id}/inventory` | operator | The commissioning report |
| `POST /servers/{id}/lifecycle` | admin | Generic transition |
| `POST /servers/{id}/zone` | admin | Set the zone |
| `POST /allocate`, `POST /servers/{id}/release`, `GET /zones` | operator | Allocation |
| `POST /servers/{id}/deploy` | admin | Image or installer deploy |
| `GET /images`, `POST /images`, `DELETE /images/{name}` | operator, admin | OS image catalog |
| `GET /profiles`, `PUT /profiles`, `DELETE /profiles/{name}`, `POST /provision` | operator, admin | Profiles and one-step provisioning |
| `GET /boot/dnsmasq.conf` | admin | proxyDHCP and TFTP configuration |
| `GET /boot/ipxe`, `GET /boot/commission.sh`, `GET /boot/deploy.sh` | none | Boot service (public by design) |
| `GET /boot/plan/{id}/{token}`, `POST /deploy/{id}/complete` | token | Image deploy plan and result |
| `GET /boot/install/{id}/{token}/{name}` | token | Installer documents |
| `POST /commissioning/{id}/report` | token | Commissioning report |
| `GET /dist/images/{name}` (not under `/baremetal`) | none | Images, with `Range` |
| `GET /ws/v1/baremetal/{id}/sol?token=` (WebSocket) | token | Serial console |

Enrolment tokens (`POST /api/v1/enrollment/tokens`) also accept `cluster_id`, `site`, `rack`, `tags`, `networks` and
`storage_pools`.

## machinactl

| Command | Does |
| --- | --- |
| `metal list`, `metal zones` | Servers with lifecycle, zone, power, MAC, BMC and error; counts per zone |
| `metal add HOSTNAME --bmc ADDR` | Register (password from `MACHINA_BMC_PASSWORD`) |
| `metal bmc-set ID --type T --address A` | Attach or change BMC details |
| `metal import --csv FILE` | Bulk registration |
| `metal discover --cidr NET [--import]` | Scan (credentials from `MACHINA_BMC_CREDENTIALS`) |
| `metal commission ID`, `metal inventory ID` | Commissioning and its report |
| `metal power ID on\|off\|cycle\|reset` | Power through the BMC |
| `metal allocate`, `metal release ID` | Allocation |
| `metal deploy ID --os OS --key KEY` | Deploy |
| `metal image list\|catalog\|add\|rm` | OS images |
| `metal profile list\|add\|rm`, `metal provision --profile P --count N` | Profiles |
| `metal dnsmasq --subnet NET` | proxyDHCP and TFTP configuration |
| `bootstrap` | Public URL, join listener, publish the agent, wait for health |
| `host add USER@HOST...` or `host add --file F` | Join nodes over SSH, in parallel, with labels |
| `dist publish`, `dist publish-metal`, `dist status` | Publish the agent and the ramdisk and ISOs |

## Configuration

Controller environment variables (set them in `/etc/default/machina-platform`).

| Variable | Default | Purpose |
| --- | --- | --- |
| `MACHINA_API_KEY_MASTER_KEY` | unset | Encrypts BMC and fencing passwords at rest; unset means plaintext |
| `MACHINA_PUBLIC_URL`, `MACHINA_CONTROLLER_TLS_ADDR` | see environment | What machines are told to call; the pinned join listener |
| `MACHINA_DIST_DIR` | `/var/lib/machina/dist` | Ramdisk, installer media, ISOs and images |
| `MACHINA_BMC_VERIFY_TLS`, `MACHINA_BMC_CA_FILE` | off, unset | Verify Redfish certificates (BMCs ship self-signed) |
| `MACHINA_BMC_POLL_SECS` | `0` | Poll BMC power state (minimum 30; 0 is off) |
| `MACHINA_BMC_SCAN_ANY` | off | Allow discovery outside private ranges |
| `MACHINA_BMC_LIBVIRT_URI`, `MACHINA_BMC_LIBVIRT_PREFIX`, `MACHINA_BMC_LIBVIRT_ANY` | `qemu:///system`, `metal-test-`, off | The libvirt driver, for labs |
| `MACHINA_IPMITOOL` | `ipmitool` | The `ipmitool` binary |
| `MACHINA_METAL_AUTO_ENROLL` | off | Enrol unknown PXE MACs |
| `MACHINA_METAL_AUTODEPLOY_SECS` | `60` | Profile top-up interval; 0 is off |
| `MACHINA_METAL_NO_AUTO_POWER` | off | Do not reset or power on after setting the boot override |
| `MACHINA_METAL_ALLOW_HTTP_JOIN` | off | Allow deploys without the TLS join listener (lab only) |
| `MACHINA_METAL_IMAGE_MAX_BYTES` | 20 GiB | Largest image an import may download |
| `MACHINA_METAL_IPXE_ISO_URL` | derived | iPXE ISO URL for virtual-media boots |
| `MACHINA_METAL_RHEL_REPO` | unset | Package repository for kickstart installs |
