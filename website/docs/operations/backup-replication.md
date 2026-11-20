---
sidebar_position: 12
title: Off-site backup replication
description: Copy backups to an S3-compatible store at another site, verify them, and pull one back for a restore.
---

# Off-site backup replication

A backup that lives only on the host it was taken on does not survive losing the site. Replication copies completed backups to an
S3-compatible store (a bucket at your second site, or any S3 service), checks the copy, and records it. It replicates and verifies;
it does **not** fail anything over. Restoring at the other site is a manual runbook, below.

## How it works

The copy is made by the **agent on the host that holds the backup**, with that host's `aws` CLI and credentials, so the data never
passes through the controller. A single-disk backup is a file (`aws s3 cp`); a multi-disk backup is a directory (`aws s3 sync`).
After the copy the agent lists the store and compares every object's size with the file on disk; only a match is marked
`verified`. It also stores a SHA-256 **checksum manifest** next to the copy (`<name>.manifest.json`), so a later fetch can prove the
data intact end to end, not just by size. The agent will only read paths inside `MACHINA_BACKUP_DIR`, and bucket, prefix and endpoint are validated so none can
act as an `aws` flag.

You need: the `aws` CLI and credentials on the hosts that take backups (the same requirement as the existing S3 upload), and a
backup target of kind `s3`:

```bash
curl -sk -X POST https://CTL:5093/api/v1/backup-targets -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"dr-site","kind":"s3","config_json":{"bucket":"dr-backups","prefix":"site-a","endpoint_url":"https://s3.dr.example"}}'
```

`prefix` defaults to `machina`; `endpoint_url` is only for S3-compatible stores.

## Use it

```bash
# One backup
curl -sk -X POST .../api/v1/backups/<backup-id>/replicate -d '{"target_id":"<uuid>"}'
curl -sk        .../api/v1/backups/<backup-id>/replicas          # status: running | verified | failed

# Everything new: each VM's newest completed backup without a verified or running copy (run it from cron)
curl -sk -X POST .../api/v1/backup-replication/run -d '{"target_id":"<uuid>","limit":10}'

# Or let the controller do it: set "auto_replicate": true in the target's config_json. Every 5 minutes it copies the
# newest not-yet-replicated backups, keeping at most 3 copies running per target.

# Overview
curl -sk ".../api/v1/backup-replicas?target_id=<uuid>&status=failed"
```

A failed copy is retried by calling replicate (or `run`) again. Copies run in the background. A controller restart during one leaves
it `running`; after 6 hours calling replicate treats it as abandoned and starts it again.

## Restore at the other site (manual)

1. Pull the verified copy onto a host at the second site (the host needs the `aws` CLI and credentials for the store):
   `curl -sk -X POST .../api/v1/backup-replicas/<replica-id>/fetch -d '{"host_id":"<uuid>"}'`. It is placed under
   `<backup dir>/fetched/<replica-id>-<name>` and verified against the store and, when the copy has a manifest, against its
   SHA-256 checksums; the replica's `fetch_status`, `fetch_path` and `fetch_message` show the result. A copy made before checksums
   existed is accepted on size alone and says so.
2. A single-disk backup is a qcow2 file: `qemu-img check` it, then define a VM on it (`virt-install --import --disk <path> ...`, or
   copy it into a storage pool and use Import VM). A multi-disk backup is a directory with `manifest.json` and one qcow2 per disk.
3. Start the VM, check the guest, then repoint DNS or load balancers.

This path is only as automatic as you make it: Machina does not detect a site loss, choose what to restore, or start anything.

## Limits

- S3 and S3-compatible targets only. Atlas backups are protected by Atlas, not copied here.
- The backup must still be on the host that took it (not pruned, and the VM has not moved to another host since).
- Upload is verified by object size; the checksum manifest is what proves the bytes on a fetch. Copies made before manifests existed
  are size-verified only (the Copies list shows which). Run `qemu-img check` after a fetch as well.
- **Retention is opt-in.** Set `"retain": N` (1 to 365) in an S3 target's config and the controller keeps the newest N verified copies
  per VM and deletes older ones from the store (at most 5 per pass, only under the target's own prefix, the manifest included).
  A copy whose local backup was already pruned has lost its record (it follows the backup) and is not seen: use a lifecycle rule on
  the bucket for those.
- Automatic replication checks every 5 minutes (not the instant a backup finishes), copies at most 3 at a time per target, has no bandwidth limit, and there is no web page yet.
- Run on a real host against an S3-compatible mock (moto), not AWS (2026-10-22): copy, checksum, fetch back and `qemu-img check` all passed. A real AWS bucket, `auto_replicate` and `retain` have not been run: see claims C64.
