---
sidebar_position: 6
title: Migration waves
description: Plan a batch of VMs, start them a few at a time through Transiva, follow each job and keep a cutover checklist.
---

# Migration waves

[Migrating VMs in](./migrate-vms-in.md) covers one VM at a time. A **wave** is the plan for many: an ordered list of VMs, how many may
run at once, the state of each Transiva job, and a cutover checklist per VM. Transiva and h2kvm still do the conversion; Machina adds
the plan, the pacing and the record. Waves need the [Transiva gateway](../reference/transiva-api.md) configured. All calls need the
`operator` role and are audited.

## Plan a wave

Each item is a VM name and the Transiva migration config for it, passed to Transiva's `/migrations/submit` unchanged. Configs are
stored encrypted (with `MACHINA_API_KEY_MASTER_KEY` set) because they can name source credentials, and are never returned by the API.

```bash
curl -sk -X POST https://CTL:5093/api/v1/migration-waves -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{
    "name": "wave-1-web", "planned_at": "2026-11-07",
    "items": [
      {"name": "web-01", "config": {"command": "vsphere", "vm": "/dc1/vm/web-01", "output_dir": "/var/lib/libvirt/images"}},
      {"name": "web-02", "config": {"command": "vsphere", "vm": "/dc1/vm/web-02", "output_dir": "/var/lib/libvirt/images"}}
    ]}'
```

## Run it

**Start** a wave and the controller advances it for you every minute: it polls the jobs already started, then starts the next
planned items, never more than `max_parallel` at once (1 to 20, default 3), and stops by itself when nothing is left. **Pause**
stops the automatic advancing (jobs already running in Transiva keep running). You can also drive it by hand with **advance**,
which does one poll-and-start pass.

```bash
curl -sk -X POST https://CTL:5093/api/v1/migration-waves/<id>/start -H "Authorization: Bearer $TOKEN" \
  -d '{"max_parallel": 2}'          # the controller now advances it
curl -sk -X POST .../migration-waves/<id>/pause     # stop advancing on its own
curl -sk -X POST .../migration-waves/<id>/advance   # or one pass by hand
```

An item moves `planned` → `submitted` → `running` → `completed` (or `failed`, `cancelled`) with Transiva's phase and percent. The wave
is `running` while anything is in flight, `done` when every item completed, and `attention` when something failed and nothing is
running. A failed item stays failed; fix it in Transiva and add it to another wave. `POST …/cancel` cancels the jobs in flight and
marks unfinished items cancelled.

## Cutover checklist

After an item completes, record where it stands: `ready` (verified on Machina, waiting for the maintenance window) or `cut_over`
(switched), with a note.

```bash
curl -sk -X POST .../migration-waves/<id>/items/<item-id>/cutover -d '{"state":"ready","note":"verified, DNS TTL lowered"}'
```

This is a record, not an action: Machina does not stop the source VM or change DNS.

## Limits

- Automatic advancing runs on the leader controller once a minute; a controller restart resumes started waves.
- Each item is one Transiva job. Dependencies between VMs (database before app) are expressed only by order and wave.
- No web page yet.
- Not yet run against a live Transiva: see claims C62.
