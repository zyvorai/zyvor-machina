# Bare-metal provisioning (Metal as a Service)

How Machina takes a rack of bare machines to joined, networked, ready hypervisor hosts:
find their BMCs, commission them, keep a pool, deploy an OS image, and join them to the
fleet, with no hand-copied media. Everything runs from the controller and is local-first:
no cloud service, and no internet at deploy time once images are imported.

Status of every claim in this guide is in [claims.md](claims.md) (C49 to C51). Short
version: the whole flow has run end to end on a lab host against **virtual machines and
BMC emulators** (sushy-tools for Redfish, VirtualBMC for IPMI). Nothing here has run on
a physical server or a physical BMC yet; see [Verification status](#verification-status).

Contents: [Concepts](#concepts) · [Quick start](#quick-start) · [Registering servers](#registering-servers) ·
[BMC control](#bmc-control) · [Commissioning](#commissioning) · [Boot service](#boot-service) ·
[OS images](#os-images) · [Allocation and deploy](#allocation-and-deploy) ·
[Profiles and one-step provisioning](#profiles-and-one-step-provisioning) ·
[Joining the fleet](#joining-the-fleet) · [Machines that already have an OS](#machines-that-already-have-an-os) ·
[Console](#console-serial-over-lan) · [API reference](#api-reference) · [CLI reference](#cli-reference) ·
[Configuration](#configuration) · [Security model](#security-model) · [Troubleshooting](#troubleshooting) ·
[Verification status](#verification-status) · [Not done](#not-done)

## Concepts

A **server** is a bare-metal machine Machina knows about: a hostname, the address and
credentials of its BMC, the MAC of the NIC it network-boots from, and a zone. Two things
about it are tracked separately:

- **`state`** is power: `registered`, `powered_on`, `powered_off`, `rebooting`. It is what
  the BMC reported or what the last command asked for (see [power state](#power-and-state)).
- **`lifecycle`** is where the machine is in provisioning.

```
            commission                       allocate                 deploy
   new ───────────────▶ commissioning ──▶ ready ─────────▶ allocated ─────────▶ deploying ──▶ deployed
    ▲                      │  │             ▲  ▲ │                │                │            │
    │                      │  └─ failed ────┘  │ └── release ─────┘                └─ failed    │
    └──────────────────────┘                   └────────────── release ──────────────────────────┘
                                                            ready ──▶ retired ──▶ new
```

Allowed moves (anything else is HTTP 409): `new→commissioning`;
`commissioning→ready|failed|new`; `ready→allocated|commissioning|retired`;
`allocated→deploying|ready`; `deploying→deployed|failed`; `deployed→ready|retired`;
`failed→commissioning|new|retired`; `retired→new`. Transitions are compare-and-set, so two
callers cannot both win.

The pieces:

| Piece | What it is | Where |
|---|---|---|
| BMC drivers | Redfish, IPMI, libvirt (lab VMs), fake | `controller/src/engine/baremetal_bmc.rs` |
| Registry and lifecycle | servers, state machine, commissioning token, inventory | `baremetal.rs`, `baremetal_lifecycle.rs` |
| Boot service rendering | proxyDHCP/TFTP config, iPXE scripts, ramdisk scripts | `baremetal_boot.rs`, `contrib/metal/` |
| Ramdisk | kernel + initrd + squashfs a machine boots to commission or write an image | `scripts/metal/build-image.sh` |
| Image catalog | verified OS images (catalog or URL) | `baremetal_images.rs` |
| Allocation, deploy | zones, smallest-fit claim, installer or image deploy | `baremetal_alloc.rs`, `baremetal_deploy.rs` |
| Profiles | "what a machine of this kind gets", one-step provision, auto top-up | `baremetal_profiles.rs` |
| Discovery, import | BMC scan, bulk registration | `baremetal_discover.rs` |
| Console | IPMI serial-over-LAN bridged to a WebSocket | `controller/src/baremetal_sol.rs` |

How a machine gets from "powered on in a rack" to "host":

```
 BMC ──(Redfish/IPMI)── controller ──(boot override + power)──▶ machine
                                                                   │ PXE (DHCP+TFTP)  or  BMC virtual media (iPXE ISO)
                                                                   ▼
                                       iPXE ─ GET /boot/ipxe?mac=… ─▶ controller (one-time token)
                                                                   │
                         commissioning:  ramdisk ─ POST report ───▶ controller ─▶ lifecycle ready
                         deploy (image): ramdisk ─ GET plan, image ─▶ writes disk, seeds cloud-init ─▶ reboot
                                                                   │ first boot: install.sh --token (pinned TLS)
                                                                   ▼
                                       POST /hosts/join ─▶ host online, validated, site/tags/networks/pools applied
```

## Quick start

On the controller host (run `machinactl` as root there). Replace the addresses and keys.

```bash
# 1. Make the controller ready to enrol nodes: public URL, pinned-TLS join listener, agent published.
sudo machinactl bootstrap --public-url https://ctl.example:5093 --tls-addr 0.0.0.0:5094

# 2. Build and publish the ramdisk (on a Linux build host; takes a few minutes, needs mkosi).
sudo scripts/metal/build-image.sh /var/tmp/metal-out
sudo machinactl dist publish-metal --dir /var/tmp/metal-out

# 3. Import an OS image (downloaded by the controller, verified against the upstream checksum).
machinactl metal image add ubuntu-26.04

# 4. Find and register the BMCs (credentials never on a command line).
export MACHINA_BMC_CREDENTIALS="root:calvin,ADMIN:ADMIN"
machinactl metal discover --cidr 10.20.0.0/24 --import --zone rack2
#    …or machinactl metal import --csv servers.csv

# 5. Commission every new machine (boots the ramdisk, reports hardware, lands in `ready`).
machinactl metal list
machinactl metal commission <id>

# 6. Say what a machine of this kind gets, then ask for N of them.
machinactl metal profile add web --image ubuntu-26.04 --key "ssh-ed25519 AAAA… ops" --zone rack2 --cores 16
machinactl metal provision --profile web --count 8

# 7. Watch them join.
machinactl host list
```

Each step is explained below. PXE needs a boot host on the machines' network (see
[Boot service](#boot-service)); if the machines cannot PXE, register them with
`boot_method: virtual_media` instead ([virtual media](#virtual-media-no-dhcptftp-needed)).

## Registering servers

```
POST /api/v1/baremetal/servers   (admin)
{ "hostname": "rack2-u14", "bmc_address": "10.20.0.14", "bmc_type": "redfish",
  "bmc_username": "admin", "bmc_password": "…", "mac_address": "aa:bb:cc:dd:ee:ff",
  "zone": "rack2", "boot_method": "pxe" }
```

| Field | Meaning |
|---|---|
| `bmc_type` | `redfish` (default), `ipmi`, `libvirt` (a libvirt domain on the controller host, for labs) or `fake` (no hardware: tests and demos) |
| `bmc_address` | host, `host:port`, `[v6]:port` or an `http(s)://` URL. Anything that could be read as an option or a path is refused |
| `bmc_username`, `bmc_password` | stored encrypted (AES-256-GCM) when `MACHINA_API_KEY_MASTER_KEY` is set; **never returned**. The record shows `bmc_credentials_configured` and `last_error` |
| `mac_address` | the NIC the machine PXE-boots from; how a boot request is matched to a server |
| `zone` | allocation scope (rack, row, site) |
| `boot_method` | `pxe` (default), `virtual_media` (BIOS) or `virtual_media_uefi` |

The `libvirt` driver only touches domains named `metal-test-*` unless
`MACHINA_BMC_LIBVIRT_PREFIX` or `MACHINA_BMC_LIBVIRT_ANY=1` says otherwise, so a mistyped
address cannot power off a real VM on the host.

### Attaching or changing BMC details

```
PUT /api/v1/baremetal/servers/{id}/bmc   (admin, audited)
{"bmc_type": "redfish", "bmc_address": "10.20.0.14", "bmc_username": "admin",
 "bmc_password": "…", "boot_method": "virtual_media"}
machinactl metal bmc-set <id> --type redfish --address 10.20.0.14 --user admin   # password from $MACHINA_BMC_PASSWORD
```

An auto-enrolled machine has a MAC and nothing else, so power calls are refused until its
BMC is set here. The same validation as registration applies; an empty `bmc_password` keeps
the stored one and an empty `boot_method` leaves it as it was.

### Finding and enrolling BMCs in bulk

```
export MACHINA_BMC_CREDENTIALS="root:calvin,ADMIN:ADMIN"      # at most 3 are tried per BMC
machinactl metal discover --cidr 10.20.0.0/24                 # list what answers
machinactl metal discover --cidr 10.20.0.0/24 --import --zone rack2   # and register them
machinactl metal import --csv servers.csv                     # hostname,bmc_address,bmc_type,username,password,mac,zone
```

- `POST /api/v1/baremetal/discover` probes the Redfish service root (`/redfish/v1`) on each
  address (32 at a time, 4 s timeout), then tries the credentials in order against
  `/redfish/v1/Systems`. It returns vendor, product, Redfish version and, when a credential
  worked, manufacturer, model and serial number. With `import: true` the authenticated
  ones are registered as `<prefix><address-with-dashes>` with that credential (encrypted).
- **Account lockout.** BMCs lock accounts after a few failures. At most 3 credentials are
  tried per BMC, and a scan covers at most 512 addresses. Do not point it at a range with
  unknown or shared accounts unless you accept that.
- **Scope.** Private IPv4 ranges only (`MACHINA_BMC_SCAN_ANY=1` allows others); loopback,
  link-local (cloud metadata), multicast and broadcast are never scanned. Admin only.
- `POST /api/v1/baremetal/import` (admin) takes up to 500 registrations. A row whose hostname
  or BMC address already exists is skipped; a bad row is reported and the rest go through,
  so the import can be re-run.
- The CSV holds BMC passwords: keep it `0600` and delete it afterwards.
- Redfish only for probing; an IPMI-only BMC is imported with `bmc_type=ipmi` (it cannot be probed).

## BMC control

The controller talks to the BMC itself, so these work while the machine is down.

### Power and state

```
POST /api/v1/baremetal/servers/{id}/power        (admin)    {"action": "on|off|cycle|reset", "dry_run": false}
GET  /api/v1/baremetal/servers/{id}/power-state  (operator) -> {"power_state": "on|off|unknown"}
```

- `power-state` asks the BMC (Redfish `PowerState`, `ipmitool chassis power status`,
  `virsh domstate`) and records the answer in `state`. Otherwise `state` only reflects the
  last command sent. `MACHINA_BMC_POLL_SECS` (default `0` = off, minimum 30) polls every
  server that has a BMC address.
- **Power calls are idempotent.** `on` for a running machine and `off` for a stopped one do
  nothing; `cycle`/`reset` of a stopped machine powers it on. On success `state` becomes
  `powered_on`, `powered_off` or `rebooting`; on failure the state is left alone,
  `last_error` records why, and the API returns 400.

| action | Redfish `ResetType` (first one the BMC allows) | `ipmitool chassis power` |
|---|---|---|
| on | `On`, `ForceOn` | `on` |
| off | `ForceOff`, `GracefulShutdown` | `off` |
| cycle | `PowerCycle`, `ForceRestart`, `GracefulRestart` | `cycle`, then `reset`, then off + on |
| reset | `ForceRestart`, `GracefulRestart`, `PowerCycle` | `reset` |

Redfish reads `ResetType@Redfish.AllowableValues` and refuses with the BMC's own list when
none of the candidates is allowed (many BMCs have no `PowerCycle`). Asynchronous actions
(HTTP 202 with a task monitor) are followed for up to about a minute. Requests are
retried 3 times on connection errors and 502/503/504. IPMI `cycle` falls back for BMCs
that reject it (VirtualBMC does).

### Boot device

```
POST /api/v1/baremetal/servers/{id}/boot-device  (admin)  {"device": "pxe|disk|cdrom", "persistent": false, "uefi": true}
```

Redfish sets `Boot.BootSourceOverrideTarget`/`Enabled` (checked against the BMC's allowed
values, so an unsupported target is refused before anything is sent); IPMI uses
`chassis bootdev` with `options=efiboot,persistent`. The libvirt driver cannot override
(boot order is fixed in the domain definition).

**Provisioning does this for you.** Commissioning and image deploy set the next boot to
network (or the virtual CD), then reset the machine if it is on or power it on if it is
off, because the override only matters at the next boot. When the image is written the next
boot is set to disk. A server without BMC credentials is left to its own boot order and the
note says so. `MACHINA_METAL_NO_AUTO_POWER=1` leaves the power step to you.

### What the BMC reports about the machine (Redfish)

```
POST /api/v1/baremetal/servers/{id}/bmc-info/refresh   (operator) read the BMC now, keep the snapshot
GET  /api/v1/baremetal/servers/{id}/bmc-info           (operator) the last snapshot
```

The snapshot holds identity (manufacturer, model, serial, SKU, BIOS, CPU count/model,
memory), the health rollup, temperatures, power draw, firmware versions (up to 40) and the 50
newest entries of the first log service. A section the BMC does not answer is listed under
`unavailable` instead of failing the read. A health other than `OK` raises a
`baremetal.health` event. IPMI, libvirt and fake servers have no read model. The Bare
Metal page shows it under **BMC details**.

### Virtual media (no DHCP/TFTP needed)

A server whose BMC is Redfish can reach the boot service without PXE: the BMC mounts a
small iPXE ISO as a virtual CD, the machine boots it, iPXE gets an address by DHCP and
chains to the controller with its MAC, and from there it is the same flow as PXE.

```
sudo scripts/metal/build-ipxe-iso.sh http://CONTROLLER:5093 /var/tmp/metal-out   # build host
sudo machinactl dist publish-metal --dir /var/tmp/metal-out                      # also copies the ISOs
```

- Register the server with `"boot_method": "virtual_media"` (BIOS) or `"virtual_media_uefi"`.
  Commissioning and image deploy then mount `$MACHINA_PUBLIC_URL/dist/metal-ipxe.iso` (or
  `metal-ipxe-uefi.iso`), boot the CD once, and when the image is written eject it and boot
  disk. `MACHINA_METAL_IPXE_ISO_URL` overrides the URL (a mirror the BMC can reach).
- Manually: `POST /api/v1/baremetal/servers/{id}/virtual-media` `{"action": "insert"|"eject", "url": "…"}`.
- The BMC fetches the image itself, so it must reach the controller (or the mirror) over
  http(s). An image already mounted is replaced; a BMC without an `InsertMedia` action is
  patched (`Image`, `Inserted`); one with no CD/DVD slot is refused with a clear message.
- The machine still needs DHCP for an address once iPXE runs. Only the boot-file/TFTP part
  is replaced.
- Building the ISOs needs git, make, gcc, `liblzma-dev`, mtools, xorriso and isolinux; set
  `IPXE_SRC` to use an existing iPXE checkout offline.

### Console (serial-over-LAN)

```
POST /api/v1/baremetal/servers/{id}/sol-token   (admin, audited) -> {"ws_path": "/ws/v1/baremetal/{id}/sol?token=…"}
```

Connect a WebSocket to `ws_path` (bytes in, bytes out; the web terminal's serial protocol).
The controller runs `ipmitool -I lanplus … sol activate` on a pseudo terminal and bridges
it; the BMC password goes through `IPMI_PASSWORD`, not argv, and a stale SOL session on the
BMC is deactivated first and again when the socket closes. One SOL session per BMC can be
active, so a second console replaces the first. Read-only grants are refused. Needs
`ipmitool` and util-linux `script` on the controller host, an IPMI (`bmc_type: ipmi`)
server, and SOL enabled on the BMC (`ipmitool sol set …`). Redfish-only BMCs are not
covered.

## Commissioning

Commissioning boots the ramdisk on a `new`/`ready`/`failed` machine to learn what it has.

```
POST /api/v1/baremetal/servers/{id}/commission     (admin) -> {token, report_path}
GET  /api/v1/baremetal/servers/{id}/inventory      (operator)
POST /api/v1/baremetal/commissioning/{id}/report   (bearer token, no login)
POST /api/v1/baremetal/servers/{id}/lifecycle      (admin) {"to": "retired"}
```

The report is a JSON object: `cpu_cores`, `memory_mib`, `disks[]` (name, size, rotational,
removable, model, serial), `nics[]` (name, MAC, link, speed), `dmi{}` (vendor, product,
serial, BIOS) and `tests[{name, passed, detail}]` (cpu, memory, disks, network; there is no
memory or disk burn-in). Any failed test moves the server to `failed`, otherwise to
`ready`; its cores and memory are copied onto the server. The token is single-use, only its
SHA-256 is stored, and the report is limited to 256 KiB.

**Auto-enrol** (`MACHINA_METAL_AUTO_ENROLL=1`, off by default): a PXE-booting machine with
an unknown MAC is recorded as `metal-<mac>` and commissioned straight away, so a newly
racked machine reaches `ready` with no registration. It has no BMC details yet, so power
calls are refused until you set them. With it on, any machine that can reach the boot
service and PXE-boots is enrolled: enable it only on a provisioning network you control.

## Boot service

For PXE the controller renders the pieces and a site's boot host runs them.

```
GET /api/v1/baremetal/boot/dnsmasq.conf?subnet=10.20.0.0&tftp_root=/srv/tftp&interface=eth1  (admin)
GET /api/v1/baremetal/boot/ipxe?mac=aa:bb:cc:dd:ee:ff     (no login: iPXE fetches it)
GET /api/v1/baremetal/boot/commission.sh                  (no login: the ramdisk runs it)
GET /api/v1/baremetal/boot/deploy.sh                      (no login: the ramdisk runs it)
```

- `dnsmasq.conf` is proxyDHCP + TFTP (`port=0`, `dhcp-range=<subnet>,proxy`). It never hands
  out addresses, so it sits beside the site's DHCP server. Plain PXE clients get
  `undionly.kpxe` / `ipxe.efi` / `ipxe-arm64.efi` from `tftp_root` (you supply those iPXE
  binaries); iPXE clients are chained to `/boot/ipxe`.
- `/boot/ipxe` answers by MAC. A registered machine that is `commissioning` gets the
  commissioning ramdisk; one that is `deploying` (and not yet written) gets the installer or
  the image-deploy ramdisk; each with a fresh one-time token on the kernel command line.
  Any other machine gets `exit` and boots its own disk.
- **The ramdisk** is three files in `MACHINA_DIST_DIR`, served from `/dist/`: `metal-vmlinuz`,
  `metal-initrd`, `metal-rootfs.squashfs`. Build it with `scripts/metal/build-image.sh`
  (mkosi, Ubuntu noble, Linux build host only; about 15 MB / 118 MB / 920 MB) and publish with
  `machinactl dist publish-metal`, which also keeps their checksums in `SHA256SUMS` across
  agent publishes. It pins the controller CA by the SHA-256 fingerprint on the kernel command
  line before trusting it (`machina-metal-run`), so a self-signed controller works.
- The squashfs is fetched into RAM by the live boot, and an image is staged in RAM too, so a
  machine being imaged needs free RAM of about the image size plus roughly 1.7 GiB.

## OS images

Images are imported once and then written to disks by image deploys.

```
machinactl metal image catalog                       # names you can import
machinactl metal image add ubuntu-26.04              # catalog image, checksum from upstream
machinactl metal image add corp-ubuntu --url https://mirror.corp/u.img --checksum <hex>
machinactl metal image list
```

Catalog: `ubuntu-26.04`, `ubuntu-26.04-arm64`, `ubuntu-24.04`, `ubuntu-22.04`, `debian-12`,
`debian-13`, `rocky-9`, `almalinux-9` (cloud images from the distributions' own servers).
The download happens in the controller, in the background (`status: downloading` →
`ready`, or `failed` with the reason).

- An image is **never kept unverified**: the checksum comes from the upstream checksum file
  (GNU or BSD layout, SHA-256 or SHA-512) or from you; a mismatch deletes the file and fails
  the import. A URL import with neither a `checksum` nor a `sums_url` is refused.
- A dropped transfer is retried (3 attempts). A checksum mismatch is final.
- Air-gapped: import from an internal mirror by URL with an explicit checksum.
- Files live in `$MACHINA_DIST_DIR/images/` and are served at `/dist/images/NAME` with `Range`
  support. Limit per image: `MACHINA_METAL_IMAGE_MAX_BYTES` (default 20 GiB).
- API: `GET/POST /api/v1/baremetal/images`, `DELETE /api/v1/baremetal/images/{name}`.

## Allocation and deploy

```
POST /api/v1/baremetal/allocate                   (operator) {"zone":"a","min_cpu_cores":16,"min_memory_mib":65536}
POST /api/v1/baremetal/servers/{id}/release       (operator) allocated|deployed -> ready
POST /api/v1/baremetal/servers/{id}/zone          (admin)    {"zone":"rack2"}
GET  /api/v1/baremetal/zones                      (operator) counts per zone
POST /api/v1/baremetal/servers/{id}/deploy        (admin)
```

Allocation takes the smallest `ready` server that satisfies the request (an empty `zone`
means any), so big machines stay free for big requests. The claim is a compare-and-set;
losing a race retries with the next candidate. No match is 409. `allocated_to` records the
requesting user.

### Image deploy (recommended)

`deploy` with `"os": "<image name>"` and at least one SSH key. The server must be allocated,
have a `mac_address` and have been commissioned (the inventory picks the disk).

```
{"os": "ubuntu-26.04", "ssh_authorized_keys": ["ssh-ed25519 AAAA… ops"], "timezone": "UTC"}
```

1. The machine boots the ramdisk (PXE or virtual media).
2. `contrib/metal/deploy.sh` fetches the plan (`/api/v1/baremetal/boot/plan/{id}/{token}`),
   downloads the image **into RAM**, verifies the checksum, and only then `qemu-img convert`s
   it to the target disk.
3. Target disk: the smallest non-removable disk that holds the image (8 GiB floor).
   `?disk=sdb` (or a serial) on the plan URL overrides.
4. It writes a cloud-init NoCloud seed (`/var/lib/cloud/seed/nocloud`) onto the image's root
   filesystem: key-only login for `machina` (passwordless sudo), hostname, timezone, and a
   first-boot script running the join command. cloud-init also grows the root partition.
5. It reports `written` (the machine is no longer offered the ramdisk), ejects virtual
   media and sets the next boot to disk, then reboots. A failure is reported as `failed`
   (server → `failed`, reason in `last_error`, stored document deleted).
6. The machine boots the OS, runs the join, and when it joins under the registered hostname
   the server becomes `deployed`.

The image must contain cloud-init (all catalog images do).

### Installer deploy

`os` may instead be `ubuntu-22.04`, `ubuntu-24.04` (autoinstall), or `rhel-9`, `rocky-9`,
`almalinux-9` (kickstart), for machines that must be installed by the distribution's own
installer. You supply the media in `MACHINA_DIST_DIR`: `install-<os>-vmlinuz`,
`install-<os>-initrd`, and for Ubuntu `install-<os>.iso`; for kickstart set
`MACHINA_METAL_RHEL_REPO` to your mirror URL. The document is served at
`/api/v1/baremetal/boot/install/{id}/{token}/user-data|meta-data|vendor-data|ks.cfg`; the
token is minted at PXE time. Inputs (hostname, timezone, keys, join command) are validated
before rendering, and a value that could break out of the document is refused.

## Profiles and one-step provisioning

A profile is "what a machine of this kind gets": image, SSH keys, zone, minimum size.

```
machinactl metal profile add web --image ubuntu-26.04 --key "ssh-ed25519 AAAA… ops" --zone rack2 --cores 16
machinactl metal provision --profile web --count 8      # allocate 8 ready machines and deploy each
machinactl metal profile add web --image ubuntu-26.04 --key … --auto --count 8   # keep 8 deployed
```

- `provision` allocates the smallest ready machine that fits, deploys it, and stops at the
  first shortage, reporting how many started and why it stopped. A machine whose deploy
  cannot start is returned to `ready`.
- With `auto_deploy` and a `desired_count`, the leader checks every
  `MACHINA_METAL_AUTODEPLOY_SECS` (default 60, `0` = off) and tops up to the desired count
  as machines become `ready`, at most 10 per pass. Machines allocated, deploying or deployed
  under the profile count toward it.
- API: `GET/PUT /api/v1/baremetal/profiles`, `DELETE /api/v1/baremetal/profiles/{name}`,
  `POST /api/v1/baremetal/provision`.

## Joining the fleet

A deployed machine joins with the controller's **pinned** join command: it checks the
script's checksum and the controller CA by fingerprint, gets a node certificate, and its
agent restarts on its own address. That needs `MACHINA_CONTROLLER_TLS_ADDR` and a
`MACHINA_PUBLIC_URL` the machine can reach; `machinactl bootstrap` sets both. Without the
listener, `deploy` is refused: over plain `http` the agent does not store its token and ends
up serving in the foreground on loopback, so cloud-init never finishes and the controller
cannot reach the agent. `MACHINA_METAL_ALLOW_HTTP_JOIN=1` lifts the check for experiments.

### Join labels (any join, not only bare metal)

`POST /api/v1/enrollment/tokens` accepts, besides `ttl_hours`:

```
{"ttl_hours": 4, "cluster_id": "<uuid>", "site": "fra1", "rack": "r12",
 "tags": ["gpu"], "networks": ["br-prod"], "storage_pools": ["ssd-pool"]}
```

- `cluster_id` (default: the first cluster), `site`, `rack` and `tags` are set on the host
  when it joins. A machine deployed from bare metal joins with `site` = the zone it was
  allocated from.
- `networks` and `storage_pools` are **existing** networks and pools, named. Once the host
  has passed validation the controller queues `network.provision` and
  `storage.pool.provision` for it. Each outcome is written to the host's join log
  (`provision` step): "ready on this host", the agent's error, or that a named resource does
  not exist (skipped). Profiles do not carry networks or pools yet.

## Machines that already have an OS

```
sudo machinactl bootstrap --public-url https://ctl.example:5093 --tls-addr 0.0.0.0:5094
machinactl host add --file hosts.txt --parallel 8 --site fra1 --rack r12 \
    --tag gpu --network br-prod --pool ssd-pool
sudo ./install.sh --agent --join https://ctl:5094 --token-file /root/join.token --ca-sha256 <fp>   # offline bundle
```

- `bootstrap` writes `MACHINA_PUBLIC_URL` and `MACHINA_CONTROLLER_TLS_ADDR` (and `_SANS`) to
  `/etc/default/machina-platform` (mode 0600), restarts the controller only if they changed,
  runs `dist publish`, and waits for health. It is idempotent and refuses malformed input.
- `host add` mints one single-use token per host (with the labels above) and runs the join
  command over SSH, up to `--parallel` at a time, revoking the token of any host that fails
  and exiting non-zero with the list. Lines of `--file` are `USER@HOST`, `#` comments. With a
  single host it keeps the interactive terminal. It runs as root on the controller (it reads
  the admin credentials from the env file).
- The offline bundle's `install.sh` joins in the same step with `--join`; it still never
  installs OS packages, so libvirt and QEMU must already be present.

## API reference

All under `/api/v1/baremetal` unless noted. `operator` and `admin` are Machina roles;
"token" means a one-time or short-lived token instead of a login.

| Method and path | Role | Purpose |
|---|---|---|
| `GET /servers`, `POST /servers` | operator, admin | list; register |
| `POST /import` | admin | bulk registration (≤500) |
| `POST /discover` | admin | scan a range for BMCs, optionally register |
| `PUT /servers/{id}/bmc` | admin | attach or change BMC type, address, login (audited) |
| `POST /servers/{id}/power` | admin | on, off, cycle, reset (`dry_run`) |
| `GET /servers/{id}/power-state` | operator | what the BMC says |
| `POST /servers/{id}/boot-device` | admin | pxe, disk, cdrom |
| `POST /servers/{id}/virtual-media` | admin | insert, eject |
| `POST /servers/{id}/bmc-info/refresh`, `GET …/bmc-info` | operator | Redfish read model |
| `POST /servers/{id}/sol-token` | admin | serial console token (audited) |
| `POST /servers/{id}/commission` | admin | start commissioning |
| `GET /servers/{id}/inventory` | operator | commissioning report |
| `POST /servers/{id}/lifecycle` | admin | generic transition |
| `POST /servers/{id}/zone` | admin | set zone |
| `POST /allocate`, `POST /servers/{id}/release`, `GET /zones` | operator | allocation |
| `POST /servers/{id}/deploy` | admin | image or installer deploy |
| `GET/POST /images`, `DELETE /images/{name}` | operator / admin | image catalog |
| `GET/PUT /profiles`, `DELETE /profiles/{name}`, `POST /provision` | operator / admin | profiles |
| `GET /boot/dnsmasq.conf` | admin | proxyDHCP/TFTP config |
| `GET /boot/ipxe`, `/boot/commission.sh`, `/boot/deploy.sh` | none | boot service (public by design) |
| `GET /boot/plan/{id}/{token}`, `POST /deploy/{id}/complete` | token | image deploy plan and result |
| `GET /boot/install/{id}/{token}/{name}` | token | installer documents |
| `POST /commissioning/{id}/report` | token | commissioning report |
| `GET /dist/…`, `GET /dist/images/{name}` (not under `/baremetal`) | none | ramdisk, media, images (Range) |
| `GET /ws/v1/baremetal/{id}/sol?token=` (WebSocket) | token | serial console |

## CLI reference

`machinactl metal …` talks to the controller through the daemon; `--fleet` is implied.

| Command | Does |
|---|---|
| `list`, `zones` | servers (lifecycle, zone, power, MAC, BMC, error); counts per zone |
| `add HOSTNAME --bmc ADDR [--type] [--user] [--mac] [--zone]` | register (password from `$MACHINA_BMC_PASSWORD`) |
| `bmc-set ID --type T --address A [--user U] [--boot-method M]` | attach or change BMC details |
| `import --csv FILE` | bulk registration |
| `discover --cidr NET [--import] [--prefix] [--zone] [--scheme] [--port]` | scan (credentials from `$MACHINA_BMC_CREDENTIALS`) |
| `commission ID`, `inventory ID` | commissioning and its report |
| `power ID on\|off\|cycle\|reset [--dry-run]` | power through the BMC |
| `allocate [--zone] [--cores] [--memory-mib]`, `release ID` | allocation |
| `deploy ID --os OS --key KEY [--timezone]` | deploy |
| `lifecycle ID STATE` | generic transition |
| `image list\|catalog\|add\|rm` | OS images |
| `profile list\|add\|rm`, `provision --profile P [--count N]` | profiles and one-step provisioning |
| `dnsmasq --subnet NET [--interface] [--tftp-root]` | proxyDHCP/TFTP config |

Related: `machinactl bootstrap`, `machinactl host add USER@HOST… | --file`, and
`machinactl dist publish | publish-metal | status`.

## Configuration

Environment variables of the controller (set them in `/etc/default/machina-platform`; the
full table with defaults is in [handbook/admin-configuration.md](handbook/admin-configuration.md)).

| Variable | Effect |
|---|---|
| `MACHINA_API_KEY_MASTER_KEY` | encrypts BMC and fencing passwords at rest (AES-256-GCM); unset = plaintext |
| `MACHINA_PUBLIC_URL`, `MACHINA_CONTROLLER_TLS_ADDR` | what machines are told to call; the pinned join listener |
| `MACHINA_DIST_DIR` | ramdisk, installer media, ISOs, images (default `/var/lib/machina/dist`) |
| `MACHINA_BMC_VERIFY_TLS`, `MACHINA_BMC_CA_FILE` | verify Redfish certificates (off by default: BMCs ship self-signed) |
| `MACHINA_BMC_POLL_SECS` | poll BMC power state (0 = off) |
| `MACHINA_BMC_SCAN_ANY` | allow discovery outside private ranges |
| `MACHINA_BMC_LIBVIRT_URI`, `_PREFIX`, `_ANY` | the libvirt (lab) driver |
| `MACHINA_IPMITOOL` | `ipmitool` binary for IPMI power and SOL |
| `MACHINA_METAL_AUTO_ENROLL` | enrol unknown PXE MACs (off) |
| `MACHINA_METAL_AUTODEPLOY_SECS` | profile top-up interval (60; 0 = off) |
| `MACHINA_METAL_NO_AUTO_POWER` | do not reset/power on after setting the boot override |
| `MACHINA_METAL_ALLOW_HTTP_JOIN` | allow deploys without the TLS join listener (lab) |
| `MACHINA_METAL_IMAGE_MAX_BYTES` | largest image an import may download (20 GiB) |
| `MACHINA_METAL_IPXE_ISO_URL` | iPXE ISO URL for virtual-media boots |
| `MACHINA_METAL_RHEL_REPO` | package repository for kickstart installs |

## Security model

- **BMC credentials** are encrypted at rest when a master key is set, never returned by the
  API, and reach `ipmitool` through the environment (`IPMI_PASSWORD`), never argv. The
  fencing password in `hosts.ipmi_password` is encrypted the same way (existing plaintext
  rows are rewritten at controller start once a key is configured).
- **Addresses and names** are validated before they reach a command line or a document;
  `ipmitool` and `script` arguments are quoted, and a quote in a host or user is refused.
- **Unauthenticated endpoints** (`boot/ipxe`, `boot/commission.sh`, `boot/deploy.sh`, the
  report, the plan, the installer documents) are gated by single-use or short-lived tokens
  and rate-limited. Anyone on the provisioning network who knows a commissioning machine's
  MAC can fetch its boot script, as with any PXE setup: keep the provisioning VLAN to the
  machines being provisioned (the `BareMetalPxe` firewall profile). Auto-enrol widens that:
  any machine that can PXE-boot is enrolled.
- **Images** are verified (checksum) before they are kept and again in the ramdisk before a
  byte reaches the disk; the ramdisk pins the controller CA by fingerprint.
- **Deployed machines** have no password login: keys only, root disabled, `machina` has
  passwordless sudo.
- **Discovery** can lock BMC accounts (3 attempts per BMC, 512 addresses, private ranges).
- **Serial console** tokens are minted by admins only and audited; the grant is bound to
  one server and refused for read-only users.

## Troubleshooting

| Symptom | Likely cause and fix |
|---|---|
| `deploy` is refused with "TLS join listener is not enabled" | run `machinactl bootstrap` (sets `MACHINA_CONTROLLER_TLS_ADDR`), or set it by hand |
| Machine PXE-boots but exits to disk | its MAC is not registered, or the server is not `commissioning`/`deploying`; check `machinactl metal list` and the MAC format |
| Machine never reaches the ramdisk although the boot override succeeded | no power step (BMC unreachable: see `last_error`), or the firmware ignores the override; try `metal power ID cycle`, or `boot_method: virtual_media` |
| Ramdisk says "controller CA fingerprint mismatch" | the fingerprint on the kernel command line is the controller's CA at the moment the iPXE script was served; a mismatch means the ramdisk reached a different controller or a proxy re-terminating TLS: check `MACHINA_PUBLIC_URL` and anything in between |
| `image add` ends `failed: … mismatch` | the file did not match the checksum; the file is deleted; retry or check the mirror |
| `deploy` fails "not enough RAM to stage the image" | give the machine more RAM or use a smaller image |
| Server stays `deploying` after the OS is up | it did not join: check the node's `cloud-init status`, `journalctl -u machina-agent`, and that it can reach the TLS listener; the join log shows each step |
| Host joined but `validation failed` | the agent is not reachable on its address (firewall on 50051, or the join ran over plain http); see the host's join events |
| `discover` finds BMCs but none accept a credential | wrong credentials, or the account is locked (it stops after 3 tries per BMC) |
| `power cycle` fails on an IPMI BMC | handled: the driver falls back to `reset`, then off + on; the response says which |
| `virtual-media` "no CD/DVD virtual media" | the BMC exposes none for this system; use PXE |
| `bmc-info` returns mostly `unavailable` | the BMC has no Chassis/Thermal/Power/UpdateService/LogService resources under the standard paths; OEM-only data is not read |
| SOL console closes at once | another session holds the BMC's SOL payload (it is deactivated and retried at connect), SOL is disabled on the BMC, or `script`/`ipmitool` is missing |

## Verification status

What has actually run (details and exact wording in [claims.md](claims.md)):

| Area | Run on the lab host (VMs and emulators) | Not run |
|---|---|---|
| Redfish power, state, boot override | sushy-tools (libvirt-backed): idempotent power, `ForceRestart` fallback, override | physical BMCs (iDRAC, iLO, Supermicro, Lenovo, OpenBMC) |
| IPMI power, state, boot device, port | VirtualBMC: all of it, including the `cycle` fallback | physical BMCs; `ipmitool` other than 1.8.19 |
| Redfish read model | mock Redfish server only | any emulator or hardware |
| Virtual media | sushy-tools: mount, boot CD (BIOS and UEFI ISOs), commission, image deploy with eject and boot from disk, join | physical virtual media (HTTPS-only URLs, one-mount limits, write protection vary by vendor) |
| PXE → commission → ready | VM, real iPXE, real ramdisk | physical NICs and firmware |
| Image deploy and join | VM (BIOS and UEFI firmware): Ubuntu 26.04 written, booted, joined over pinned TLS, validated | physical disks and firmware |
| Image catalog | 7 catalog images imported and checksum-verified (BSD layout, SHA-512) | `ubuntu-26.04-arm64` |
| `bootstrap`, `host add --file --parallel`, labels, post-join networks and pools, offline bundle `--join`, a join that fails after SSH succeeds (token revoked, exit 1) | VMs | real nodes with real networks |
| Serial console | pty round trip with a stand-in `ipmitool` | the WebSocket in a browser; a real BMC's SOL |
| Auto-enrol, `bmc-set`, profile top-up | two empty VMs: enrolled, commissioned, BMC attached, deployed by an `auto_deploy` profile, joined | with physical machines |
| Web panels (servers, images, profiles, BMC details, register form) | 13 component tests with a mocked API (`PlatformBareMetal.test.tsx`), and the page driven in headless Chrome 155 against the lab host (rows, badges, images, profiles, Commission, Provision with the refusal toast, register form) | a headed browser session by a person; the BMC details panel (no Redfish server with data was on the page) |

## Not done

- A burn-in (memory, disk) in commissioning; firmware and BIOS configuration.
- Static IP and bonding for a deployed machine (DHCP on the provisioning NIC only), disk
  layout beyond "the image as built" (image deploy) or LVM on the default disk (installer).
- Profiles that carry networks and pools; validation that a named bridge exists beyond the
  provisioning task's own result.
- Redfish-only serial console, OEM-specific sensors and logs, firmware update, RAID.
- Physical-hardware vendor profiles; a published compatibility list.
