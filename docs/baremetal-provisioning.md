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

## Not done yet

1. Boot service (proxyDHCP, TFTP, iPXE/HTTP boot) on a site agent.
2. Commissioning image that produces the report above.
3. Zones, allocation, autoinstall/kickstart rendering, first-boot join through the
   existing enrollment token.
4. Moving `hosts.ipmi_password` (fencing) to the same encrypted storage.

Claim status: C49 in [claims.md](claims.md).
