---
sidebar_position: 11
title: Desktop pools (simple VDI)
description: Clone a template VM into a pool of desktops, hand one to each user who checks out, and recycle it when they are done.
---

# Desktop pools (simple VDI)

A **desktop pool** is a stopped template VM cloned into numbered desktops (`sales-01`, `sales-02`, ...). A user **checks out** a
desktop, which powers it on if needed, and **releases** it when done. In a **non-persistent** pool (the default) the desktop is
deleted and re-cloned from the template on release, so every session starts clean. A **persistent** pool keeps the user's disk and
only shuts the desktop down.

Desktops are ordinary Machina VMs created with the existing clone, power and delete tasks, so they show up in the VM list, have
consoles, snapshots and metrics, and can be reached the usual way (RDP or the browser console). This is a pool manager, not a
connection broker: there is no gateway, no session reconnection and no per-user entitlement beyond who calls checkout.

## Use it

```bash
# 1. Build the golden image as a normal VM, shut it down, then make a pool of 10 linked clones
curl -sk -X POST https://CTL:5093/api/v1/desktop-pools -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"sales","template_vm_id":"<uuid>","size":10}'

# 2. A user takes a desktop (any authenticated user, for themselves); it is powered on
curl -sk -X POST https://CTL:5093/api/v1/desktop-pools/<id>/checkout -H "Authorization: Bearer $TOKEN"
# -> {"pool":"sales","user":"alice","desktop":{"vm_name":"sales-03","vm_id":"...","vm_state":"shutoff","state":"assigned",...}}

# 3. When done
curl -sk -X POST https://CTL:5093/api/v1/desktop-pools/<id>/release -H "Authorization: Bearer $TOKEN"

# Operators: scale, look, nudge, delete (add ?delete_vms=true to delete the desktop VMs too)
curl -sk -X POST .../desktop-pools/<id>/scale -d '{"size":20}'
curl -sk        .../desktop-pools/<id>
curl -sk -X POST .../desktop-pools/<id>/reconcile
curl -sk -X DELETE ".../desktop-pools/<id>?delete_vms=true"
```

A user holds at most one desktop per pool (checking out again returns the same one). An operator can pass `{"user":"bob"}` to check
out or release for someone else.

## States

`needs_clone` → `cloning` → `ready` → `assigned`; a released non-persistent desktop goes `recycling` while its VM is deleted, then
back through `needs_clone`. `error` shows why a clone could not be queued and is retried.

The controller reconciles every pool every 30 seconds (on the leader): it notices finished clones, re-clones recycled desktops and
removes surplus ones. **Reconcile** does the same pass on demand and is also called by create, scale, checkout and release. A
checkout with no free desktop returns 409 until a clone is `ready`.

## Limits

- **Single host.** Clones are made where the template lives, so a pool runs on one host. Use one template per host for more.
- **No Windows multi-session.** Each desktop is a full VM; Windows 11 multi-session is not available on KVM.
- **No broker.** Machina does not proxy sessions or reconnect users; give users the VM console or RDP address.
- **Any authenticated user can check out and release for themselves**, which starts a VM: restrict who can log in accordingly.
- A clone that never completes stays `cloning`; delete the stuck VM and reconcile.
- No web page yet.
- Run on one real libvirt host (2026-10-22): create, checkout, release, scale and delete, with linked clones of a stopped template that had no guest OS. Releasing a desktop used to leave its disk behind and the re-clone failed; that is fixed. A booting guest and more than one host are not shown: see claims C63.
