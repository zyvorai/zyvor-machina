# Launching instances

## What it is
The EC2-style launch path: free-form user data, key pairs by name, several instances in one call (run-instances),
instance types you can change later, and a `terminated` tombstone that stays visible for an hour after delete.

## Configure
- **Flavors** are your instance types (`POST /api/v1/flavors {name, vcpus, memory_mb, disk_gb}`).
- **Key pairs**: register a public key once (`POST /api/v1/keypairs`); machina stores public keys only, so create the pair with
  `ssh-keygen`.
- **User data**: up to 16 KB of cloud-init text, written verbatim as the NoCloud `user-data` in the guest's seed ISO. It is never
  logged. The guest image needs cloud-init (cirros and every cloud image do).

All calls below use two shell variables:

```bash
export API=https://HOST:5092/api/v1/platform/controller/api/v1   # controller API through the daemon proxy
export TOKEN=...                                                  # a JWT or API key (Settings -> API keys)
alias mc='curl -sk -H "Authorization: Bearer $TOKEN" -H "content-type: application/json"'
```

## Use
```bash
# one instance with a key pair and a script that runs at first boot
mc -X POST $API/vms/from-template -d '{"name":"web-1","template":"cirros","flavor_id":"<flavor uuid>",
  "key_name":"laptop","cloud_init_user_data":"#!/bin/sh\necho hi > /tmp/hello\n"}'

# three at once: web-1 .. web-3; fails with 409 run_instances_min_count if fewer than min_count could be created
mc -X POST $API/vms/run-instances -d '{"name":"web","template":"cirros","count":3,"min_count":2}'

# resize: stops cleanly, sets vCPUs and memory to the flavor's, starts again (a task with events)
mc -X POST $API/vms/web-1/change-type -d '{"flavor_id":"<bigger flavor uuid>"}'
```
Deleting an instance leaves a tombstone: `DescribeInstances` on the EC2 endpoint lists it as `terminated` for an hour.

## Check it works
Boot a cirros guest with a user-data script that writes to its console or serves a port, then confirm the effect. The live test
`live_test3` in the lab notes does exactly that. Unit tests: `cargo test -p machina-controller type_change_tests run_instances_tests`
and `cargo test -p machina-core meta_data_tests seed_scratch_tests`.

## Limits
`change-type` does not grow the disk, refuses instances managed by an instance group (change the launch template instead)
and refuses a change that would exceed the project quota. run-instances creates machines one after another and does not roll
back the ones already created when it fails. There is no create-and-return-the-private-key call.
