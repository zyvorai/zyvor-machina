---
sidebar_position: 5
title: Provision bare metal
description: From a BMC address to joined hypervisor hosts in seven steps.
---

# Provision bare metal

This takes machines that have no OS from a BMC address to joined hosts. Read the
[concepts](../core-concepts/bare-metal.md) first, and note the
[verification status](../core-concepts/bare-metal.md#verification-status): this has run against VMs and BMC emulators,
not yet on physical servers.

Run `machinactl` as root on the controller host. Replace the addresses and keys.

## 1. Make the controller ready to enrol nodes

```bash
sudo machinactl bootstrap --public-url https://ctl.example:5093 --tls-addr 0.0.0.0:5094
```

This writes the public URL and the pinned-TLS join listener to `/etc/default/machina-platform` (mode 0600), restarts the
controller if they changed, publishes the agent and waits for health. It is idempotent. Image deploys require the TLS
listener: over plain `http` the agent cannot reach the controller after joining.

## 2. Build and publish the boot ramdisk

On a Linux build host with mkosi:

```bash
sudo scripts/metal/build-image.sh /var/tmp/metal-out
sudo machinactl dist publish-metal --dir /var/tmp/metal-out
```

Machines that cannot PXE need the virtual-media ISOs as well:

```bash
sudo scripts/metal/build-ipxe-iso.sh https://ctl.example:5093 /var/tmp/metal-out
sudo machinactl dist publish-metal --dir /var/tmp/metal-out
```

## 3. Import an OS image

```bash
machinactl metal image catalog
machinactl metal image add ubuntu-26.04
machinactl metal image list
```

The controller downloads the image in the background and keeps it only when it matches the upstream checksum. For an
air-gapped site, import from an internal mirror by URL with an explicit checksum:

```bash
machinactl metal image add corp-ubuntu --url https://mirror.corp/u.img --checksum SHA256_HEX
```

## 4. Find and register the BMCs

```bash
export MACHINA_BMC_CREDENTIALS="root:calvin,ADMIN:ADMIN"   # at most 3 are tried per BMC
machinactl metal discover --cidr 10.20.0.0/24 --import --zone rack2
```

Or import a CSV (`hostname,bmc_address,bmc_type,username,password,mac,zone`):

```bash
machinactl metal import --csv servers.csv
```

Each server needs the MAC of the NIC it boots from; set it on registration or with `machinactl metal bmc-set`.

## 5. Commission

```bash
machinactl metal list
machinactl metal commission SERVER_ID
```

The machine boots the diagnostic ramdisk, reports its hardware and lands in `ready`. For PXE, give your boot host the
proxyDHCP and TFTP configuration the controller renders:

```bash
machinactl metal dnsmasq --subnet 10.20.0.0 --interface eth1 > /etc/dnsmasq.d/machina.conf
```

For virtual media, register the server with `"boot_method": "virtual_media"` (BIOS) or `"virtual_media_uefi"`.

## 6. Say what a machine gets, then ask for some

```bash
machinactl metal profile add web --image ubuntu-26.04 --key "ssh-ed25519 AAAA... ops" --zone rack2 --cores 16
machinactl metal provision --profile web --count 8
```

To keep a number deployed automatically:

```bash
machinactl metal profile add web --image ubuntu-26.04 --key "ssh-ed25519 AAAA... ops" --auto --count 8
```

## 7. Watch them join

```bash
machinactl host list
machinactl metal list
```

A machine goes `allocated`, `deploying`, then `deployed` when its host joins. The host's join log lists each step,
including the outcome of any networks and storage pools named on its token.

## Machines that already have an OS

```bash
machinactl host add --file hosts.txt --parallel 8 --site fra1 --rack r12 --tag gpu --network br-prod --pool ssd-pool
```

Offline, from the release bundle on the node:

```bash
sudo ./install.sh --agent --join https://ctl:5094 --token-file /root/join.token --ca-sha256 FINGERPRINT
```
