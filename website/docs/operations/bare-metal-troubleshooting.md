---
sidebar_position: 6
title: Bare-metal troubleshooting
description: Symptoms, causes and fixes for bare-metal provisioning.
---

# Bare-metal troubleshooting

Start with `machinactl metal list`: look at `LIFECYCLE`, `POWER` and `ERROR` for the server. The `ERROR` column is the
server's `last_error`, which records the last failed BMC call or provisioning step.

| Symptom | Likely cause and fix |
| --- | --- |
| `deploy` is refused with "TLS join listener is not enabled" | Run `machinactl bootstrap` (sets `MACHINA_CONTROLLER_TLS_ADDR`), or set it by hand. `MACHINA_METAL_ALLOW_HTTP_JOIN=1` lifts the check for experiments only. |
| A machine PXE-boots but exits to its disk | Its MAC is not registered, or the server is not `commissioning` or `deploying`. Check the MAC format and `machinactl metal list`. |
| A machine never reaches the ramdisk although the override succeeded | There was no power step (the BMC is unreachable: see `last_error`) or the firmware ignores the override. Try `machinactl metal power ID cycle`, or use `boot_method: virtual_media`. |
| The ramdisk reports a controller CA fingerprint mismatch | It reached a different controller, or a proxy that re-terminates TLS. Check `MACHINA_PUBLIC_URL` and anything in between. |
| `image add` ends `failed` with a mismatch | The file did not match the checksum and was deleted. Retry, or check the mirror. |
| `deploy` fails "not enough RAM to stage the image" | The image is staged in RAM so it can be verified before writing. Give the machine more RAM or use a smaller image. |
| A server stays `deploying` after the OS is up | It did not join. On the node check `cloud-init status` and `journalctl -u machina-agent`, and that the TLS join listener is reachable. The host's join log shows each step. |
| A host joined but validation failed | The agent is not reachable on its address (firewall on 50051, or the join ran over plain http). See the host's join events. |
| `discover` finds BMCs but none accept a credential | Wrong credentials, or the account is locked. It stops after three tries per BMC. |
| `power cycle` fails on an IPMI BMC | Handled: the driver falls back to `reset`, then off and on; the response says which. |
| `virtual-media` says "no CD/DVD virtual media" | The BMC exposes none for this system. Use PXE. |
| BMC details are mostly "not reported" | The BMC has no Chassis, Thermal, Power, UpdateService or LogService resources under the standard paths. OEM-only data is not read. |
| The serial console closes at once | Another session holds the BMC's SOL payload, SOL is disabled on the BMC (`ipmitool sol set`), or `script` or `ipmitool` is missing on the controller. |
| An auto-enrolled machine cannot be powered | It has no BMC yet. Attach one with `machinactl metal bmc-set`. |

## Releasing a machine

```bash
machinactl metal release SERVER_ID      # allocated or deployed -> ready
machinactl metal lifecycle SERVER_ID retired
```
