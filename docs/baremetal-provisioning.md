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

## Zones and allocation

```
POST /api/v1/baremetal/allocate            (operator) {"zone":"a","min_cpu_cores":16,"min_memory_mib":65536}
POST /api/v1/baremetal/servers/{id}/release (operator) allocated|deployed -> ready
POST /api/v1/baremetal/servers/{id}/zone   (admin)    {"zone":"rack2"}
GET  /api/v1/baremetal/zones                (operator) counts per zone
```

Allocation takes the smallest `ready` server that satisfies the request (an empty
`zone` means any), so big machines stay free for big requests. The claim is a
compare-and-set; losing a race retries with the next candidate. No match is 409.
`allocated_to` records the requesting user.

## Deploy

```
POST /api/v1/baremetal/servers/{id}/deploy   (admin)
{"os": "ubuntu-24.04", "ssh_authorized_keys": ["ssh-ed25519 AAAA… ops"], "timezone": "UTC"}
```

`allocated -> deploying`. The server needs a `mac_address`. The controller issues an
enrollment token, renders the installer document and stores it; when the machine
PXE-boots, `/boot/ipxe` hands it the installer and the document by URL.

| os | installer | notes |
|---|---|---|
| `ubuntu-22.04`, `ubuntu-24.04` | autoinstall (cloud-init NoCloud-net) | needs `install-<os>-vmlinuz`, `install-<os>-initrd`, `install-<os>.iso` in `MACHINA_DIST_DIR` |
| `rhel-9`, `rocky-9`, `almalinux-9` | kickstart | needs `install-<os>-vmlinuz`, `-initrd`, and `MACHINA_METAL_RHEL_REPO` (your mirror URL) |

- No password login: at least one SSH public key is required; root and the `machina`
  user are locked.
- First boot runs the normal join command (the TLS-pinned one when the network
  listener is on) from `/usr/local/sbin/machina-join.sh`, so the machine joins as an
  ordinary host. When it joins under the registered hostname the server becomes
  `deployed` and the stored document, which holds the enrollment token, is deleted.
  Releasing the server deletes it too.
- The document is served at `/api/v1/baremetal/boot/install/{id}/{token}/user-data`
  (`meta-data`, `vendor-data`, `ks.cfg`); `{token}` is minted at PXE time.
- Hostname, timezone, keys and the join command are validated before rendering; a
  value that could break out of the document is refused.

## OS images

Images are imported once and then written to disks by image-based deploys.

```
machinactl metal image catalog                       # names you can import
machinactl metal image add ubuntu-26.04              # catalog image, checksum from upstream
machinactl metal image add corp-ubuntu --url https://mirror.corp/u.img --checksum <hex>
machinactl metal image list
```

Catalog: `ubuntu-26.04`, `ubuntu-26.04-arm64`, `ubuntu-24.04`, `ubuntu-22.04`,
`debian-12`, `debian-13`, `rocky-9`, `almalinux-9` (cloud images from the distributions'
own servers). The download happens in the controller, in the background
(`status: downloading` -> `ready` or `failed` with the reason).

- An image is **never kept unverified**: the checksum comes from the upstream checksum
  file (GNU or BSD layout, SHA-256 or SHA-512) or from you; a mismatch deletes the file
  and fails the import. A URL import with neither a `checksum` nor a `sums_url` is refused.
- Air-gapped: import from an internal mirror by URL with an explicit checksum.
- Files live in `$MACHINA_DIST_DIR/images/` and are served at `/dist/images/NAME` with
  `Range` support, so a machine can resume a download. Limit per image:
  `MACHINA_METAL_IMAGE_MAX_BYTES` (default 20 GiB).

### Deploy an image to disk

`POST .../servers/{id}/deploy` with `"os": "<image name>"` (an imported, `ready` image)
deploys without any installer media. The server must be allocated, have a `mac_address`
and have been commissioned (the inventory picks the disk).

1. The machine PXE-boots the Machina ramdisk (built by `scripts/metal/build-image.sh`,
   published with `machinactl dist publish-metal`).
2. `contrib/metal/deploy.sh` fetches the plan
   (`/api/v1/baremetal/boot/plan/{id}/{token}`), downloads the image **into RAM**,
   verifies the checksum, and only then `qemu-img convert`s it to the target disk.
   The machine needs free RAM of about the image size plus 768 MiB; it fails with a clear
   message otherwise.
3. Target disk: the smallest non-removable disk that holds the image (8 GiB floor).
   `?disk=sdb` (or a serial) on the plan URL overrides.
4. It writes a cloud-init NoCloud seed (`/var/lib/cloud/seed/nocloud`) onto the image's
   root filesystem: key-only login for `machina`, hostname, timezone, and a first-boot
   script running the normal join command. cloud-init also grows the root partition.
5. It reports `written`; from then on `/boot/ipxe` answers `exit` for that machine, so a
   reboot falls through to the disk. The host joining marks the server `deployed`.
   A failure is reported as `failed` (server -> `failed`, reason in `last_error`).

The image must contain cloud-init (all catalog images do).

## Profiles, one-step provisioning and auto-enroll

A profile is "what a machine of this kind gets": image, SSH keys, zone, minimum size.

```
machinactl metal profile add web --image ubuntu-26.04 --key "ssh-ed25519 AAAA… ops" --zone rack2 --cores 16
machinactl metal provision --profile web --count 8     # allocate 8 ready machines and deploy each
machinactl metal profile add web --image ubuntu-26.04 --key … --auto --count 8   # keep 8 deployed
```

- `provision` allocates the smallest ready machine that fits (as `allocate` does), deploys
  it, and stops at the first shortage, reporting how many started and why it stopped.
  A machine whose deploy cannot start is returned to `ready`.
- With `auto_deploy` and a `desired_count`, the leader checks every
  `MACHINA_METAL_AUTODEPLOY_SECS` (default 60, `0` = off) and tops up to the desired count
  as machines become `ready`, at most 10 per pass. Machines allocated, deploying or
  deployed under the profile count toward it.
- **Auto-enroll** (`MACHINA_METAL_AUTO_ENROLL=1`, off by default): a PXE-booting machine
  with an unknown MAC is recorded as `metal-<mac>` and commissioned straight away, so a
  newly racked machine reaches `ready` with no registration. It has no BMC details yet,
  so power calls are refused until you set them. With it on, any machine that can reach
  the boot service and PXE-boots is enrolled and its hardware report accepted: enable it
  only on a provisioning network you control.

## Join labels (any join, not only bare metal)

`POST /api/v1/enrollment/tokens` accepts, besides `ttl_hours`:

```
{"ttl_hours": 4, "cluster_id": "<uuid>", "site": "fra1", "rack": "r12",
 "tags": ["gpu"], "networks": ["br-prod"], "storage_pools": ["ssd-pool"]}
```

- `cluster_id` (default: the first cluster), `site`, `rack` and `tags` are set on the host
  when it joins.
- `networks` and `storage_pools` are **existing** networks and pools, named. Once the host
  has passed validation the controller queues `network.provision` and
  `storage.pool.provision` for it, and logs each step in the join log. A name that does not
  exist is reported there and skipped.
- A machine deployed from bare metal joins with `site` = the zone it was allocated from.
- Not done: validation does not yet check that the bridge or pool really exists on the host
  afterwards, and `MACHINA_METAL`-style profiles do not carry networks or pools yet.

## Machines that already have an OS

```
sudo machinactl bootstrap --public-url https://ctl.example:5093   # on the controller, once
machinactl host add --file hosts.txt --parallel 8 --site fra1 --rack r12 \
    --tag gpu --network br-prod --pool ssd-pool
sudo ./install.sh --agent --join https://ctl:5094 --token-file /root/join.token --ca-sha256 <fp>   # offline bundle
```

- `bootstrap` writes `MACHINA_PUBLIC_URL` and `MACHINA_CONTROLLER_TLS_ADDR` (and
  `_SANS`) to `/etc/default/machina-platform` (mode 0600), restarts the controller only
  if they changed, runs `dist publish`, and waits for health. It prints what to run next.
- `host add` mints one single-use token per host (with the labels above) and runs the
  join command over SSH, up to `--parallel` at a time, revoking the token of any host that
  fails and exiting non-zero with the list. Lines of `--file` are `USER@HOST`, `#` comments.
  With a single host it keeps the interactive terminal as before.
- The offline bundle's `install.sh` joins in the same step with `--join`; it still never
  installs OS packages, so libvirt and QEMU must already be present.

## CLI and UI

```
machinactl metal add rack2-u14 --bmc 10.20.0.14 --user admin --mac aa:bb:cc:dd:ee:ff --zone rack2
machinactl metal commission <id>        # then PXE-boot the machine
machinactl metal inventory <id>
machinactl metal allocate --zone rack2 --cores 16
machinactl metal deploy <id> --os ubuntu-24.04 --key "ssh-ed25519 AAAA… ops"
machinactl metal release <id>
machinactl metal list | zones | power <id> on|off|cycle|reset [--dry-run]
machinactl metal dnsmasq --subnet 10.20.0.0 --interface eth1 > /etc/dnsmasq.d/machina.conf
```

The BMC password is read from `MACHINA_BMC_PASSWORD`, never an argument. The commands
talk to the controller (`--fleet` is implied). In the UI, Platform -> Bare Metal shows
lifecycle and zone next to power state, takes BMC credentials, MAC and zone on
registration, and has Commission and Release buttons.

## Not done yet

1. A commissioning image build (kernel, initrd, rootfs). The script and the contract
   are here; the image is not built or shipped.
3. Building or fetching installer media; the operator copies it into `MACHINA_DIST_DIR`.
4. Disk layout beyond LVM on the default disk, static IPs, NIC bonding.
5. Moving `hosts.ipmi_password` (fencing) to the same encrypted storage.

Claim status: C49 in [claims.md](claims.md).
