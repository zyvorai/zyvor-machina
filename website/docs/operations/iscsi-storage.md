---
sidebar_position: 13
title: iSCSI storage
description: Log in to an iSCSI target, expose its LUNs as a storage pool, and attach a LUN to a VM as a block disk.
---

# iSCSI storage

A SAN array presents LUNs over iSCSI. Machina logs in to one target through a libvirt **iSCSI pool**, lists the LUNs as volumes, and
attaches a LUN to a VM as a raw block disk. LUNs are carved out on the array, not by Machina: the pool lists and attaches them but
cannot create, resize or delete them.

This is host-level storage on the daemon. It needs `open-iscsi` (`iscsiadm`) on the host for discovery and for libvirt's login.

## Use it

```bash
# 1. Which targets does the array offer? (runs iscsiadm sendtargets on the host)
curl -sk -X POST https://HOST:5092/api/v1/host/iscsi/discover -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"host":"10.0.0.20"}'
# {"portal":"10.0.0.20:3260","targets":[{"portal":"10.0.0.20:3260","iqn":"iqn.2001-04.com.example:disk1"}]}

# 2. Log in and create the pool (CHAP optional: give both fields or neither)
curl -sk -X POST https://HOST:5092/api/v1/storage/iscsi-pools -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"san1","host":"10.0.0.20","target_iqn":"iqn.2001-04.com.example:disk1",
       "initiator_iqn":"iqn.1993-08.org.debian:01:abcd","chap_username":"machina","chap_secret":"correct-horse-12"}'

# 3. The LUNs appear as volumes of the pool
curl -sk -H "Authorization: Bearer $TOKEN" https://HOST:5092/api/v1/storage/pools/san1/volumes

# 4. Attach one to a VM (hot-plug on a running VM); detach with the normal disk detach and the same target
curl -sk -X POST https://HOST:5092/api/v1/vms/web-01/disks/lun -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"pool":"san1","volume":"unit:0:0:1","target":"vdb"}'
```

The pool autostarts, so the login survives a reboot. The CHAP secret is stored as a libvirt secret (usage `machina-iscsi-<pool>`)
and the pool refers to it by usage: it is never in the pool XML and never returned. Everything that reaches XML or `iscsiadm` is
validated first (address, IQNs, CHAP user and a 12 to 64 character secret).

A LUN is attached as `<disk type='block' device='disk'>` with `cache='none' io='native'`. Add `"readonly":true` or `"shareable":true`
(for cluster filesystems) in the request body. The pool must be an iSCSI pool, so this cannot hand a guest an arbitrary host device.

## Limits

- **One target per pool.** Create a pool per target. No multipath configuration: set up `multipathd` on the host and the LUN paths
  libvirt reports will be the multipath devices.
- **Live migration** of a VM holding a LUN needs the same LUN visible on the destination host (log the destination in to the same
  target) and `shareable` semantics your array and filesystem support. Machina does not check this for you.
- Daemon only: no controller or fleet API, no web page yet.
- Not yet run against an array: see claims C66.
