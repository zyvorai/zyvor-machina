# Bare-metal provisioning (Metal as a Service)

Status: **in progress.** Power control is live. PXE boot, commissioning, hardware
inventory, allocation and OS deploy are not implemented yet; the plan is below.
`GET /api/v1/baremetal/servers/{id}/provision` still returns a preview only.

## What works today

### Register a server with BMC credentials

```
POST /api/v1/baremetal/servers   (admin)
{ "hostname": "rack2-u14", "bmc_address": "10.20.0.14", "bmc_type": "redfish",
  "bmc_username": "admin", "bmc_password": "…" }
```

- `bmc_type` is `redfish` (default), `ipmi` or `fake`. `fake` touches no hardware
  and is meant for tests and demos.
- The password is stored encrypted (AES-256-GCM) when `MACHINA_API_KEY_MASTER_KEY`
  is set, and is never returned. The server record shows
  `bmc_credentials_configured` and `last_error`.
- `bmc_address` is a host, `host:port`, or an `http(s)://` URL. Anything that could
  be read as an option or a path is rejected.

### Power

```
POST /api/v1/baremetal/servers/{id}/power   (admin)
{ "action": "on" | "off" | "cycle" | "reset", "dry_run": false }
```

The controller calls the BMC itself, so this works while the host is down.

| action | Redfish `ResetType` | `ipmitool power` |
|---|---|---|
| on | `On` | `on` |
| off | `ForceOff` | `off` |
| cycle | `PowerCycle` | `cycle` |
| reset | `ForceRestart` | `reset` |

On success `state` becomes `powered_on`, `powered_off` or `rebooting`. On failure
the state is left as it was, `last_error` records why, and the API returns 400.
A non-`fake` server without credentials is refused rather than pretended.

Redfish uses the first `ComputerSystem` in `/redfish/v1/Systems`. BMC certificates
are self-signed, so verification is off unless `MACHINA_BMC_VERIFY_TLS=1`.
`ipmitool` must be installed on the controller host; the password is passed through
`IPMI_PASSWORD`, not argv.

## Lifecycle and commissioning

`new -> commissioning -> ready -> allocated -> deploying -> deployed -> retired`, plus
`failed`. Allowed moves: `new->commissioning`; `commissioning->ready|failed|new`;
`ready->allocated|commissioning|retired`; `allocated->deploying|ready`;
`deploying->deployed|failed`; `deployed->ready|retired`; `failed->commissioning|new|retired`;
`retired->new`. Anything else is 409. `lifecycle` is separate from `state` (power).

```
POST /api/v1/baremetal/servers/{id}/commission     (admin) -> {token, report_path}
POST /api/v1/baremetal/servers/{id}/lifecycle      (admin) {"to": "retired"}
GET  /api/v1/baremetal/servers/{id}/inventory      (operator)
POST /api/v1/baremetal/commissioning/{id}/report   (bearer token, no login)
```

The report is a JSON object: `cpu_cores`, `memory_mib`, `disks[]`, `nics[]`, `dmi{}`,
`tests[{name, passed, detail}]`. Any failed test moves the server to `failed`, otherwise
to `ready`; its cores and memory are copied onto the server. The token is single-use,
only its SHA-256 is stored, and the report is limited to 256 KiB.

## Network boot

The controller renders the pieces; a site's boot host runs them.

```
GET /api/v1/baremetal/boot/dnsmasq.conf?subnet=10.20.0.0&tftp_root=/srv/tftp&interface=eth1  (admin)
GET /api/v1/baremetal/boot/ipxe?mac=aa:bb:cc:dd:ee:ff     (no login: iPXE fetches it)
GET /api/v1/baremetal/boot/commission.sh                  (no login: the image runs it)
```

- `dnsmasq.conf` is proxyDHCP + TFTP (`port=0`, `dhcp-range=<subnet>,proxy`). It never
  hands out addresses, so it sits beside the site's DHCP server. Plain PXE clients get
  `undionly.kpxe` / `ipxe.efi` / `ipxe-arm64.efi` from `tftp_root` (you supply those
  iPXE binaries); iPXE clients are chained to `/boot/ipxe`.
- `/boot/ipxe` serves the commissioning image only to a MAC registered on a server that
  is in `commissioning`, with a fresh one-time token in the kernel command line. Any
  other machine gets `exit` and boots its own disk. Anyone on the provisioning network
  who knows a commissioning machine's MAC can fetch that script, as with any PXE
  setup: keep the provisioning VLAN to the machines being provisioned (the
  `BareMetalPxe` firewall profile).
- The image is three files in `MACHINA_DIST_DIR`, served from `/dist/`:
  `metal-vmlinuz`, `metal-initrd`, `metal-rootfs.squashfs`. The rootfs needs `python3`
  and `lsblk`. `commission.sh --print` prints the report without posting it.
- The commissioning script reports CPU, memory, disks (name, size, rotational, model,
  serial), NICs (MAC, link, speed), DMI vendor/product/serial/BIOS and four basic
  tests (cpu, memory, disks, network). It does not run a memory or disk burn-in.

Register the PXE NIC's MAC with `mac_address` (and `zone`) on the server.

## Not done yet

1. A commissioning image build (kernel, initrd, rootfs). The script and the contract
   are here; the image is not built or shipped.
3. Zones, allocation, autoinstall/kickstart rendering, first-boot join through the
   existing enrollment token.
4. Moving `hosts.ipmi_password` (fencing) to the same encrypted storage.

Claim status: C49 in [claims.md](claims.md).
