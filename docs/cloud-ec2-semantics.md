# Fleet Cloud: EC2 semantics

What behaves like EC2 today, what is enforced, and what is not. Everything here is available through the controller
API (`/api/v1/...`) and the Fleet Cloud pages; the EC2-compatible Query endpoint is described in
[cloud-ec2-api.md](cloud-ec2-api.md).

| Area | Sections |
|---|---|
| Identity | [Tags and ids](#tags-and-ids), [Ids and tag filters beyond instances](#ids-and-tag-filters-beyond-instances), [Project-scoped API keys](#project-scoped-api-keys) |
| Compute | [Instance types](#instance-types), [User data](#user-data), [Key pairs by name](#key-pairs-by-name), [Run several instances](#run-several-instances-runinstances), [Instance metadata service](#instance-metadata-service) |
| Storage | [Volumes](#volumes-delete-on-termination-and-io-limits), [Volume from snapshot](#volume-from-snapshot), [Image visibility and sharing](#image-visibility-and-sharing-ami-style) |
| Network | [Security groups](#security-groups), [Network interfaces](#network-interfaces-enis), [Elastic IPs](#elastic-ips), [NAT gateway](#nat-gateway-for-private-subnets), [Deleting subnets](#deleting-subnets), [Load balancer health checks](#load-balancer-health-checks) |
| Scaling and metrics | [Alarms](#alarms), [Metric statistics](#metric-statistics), [Deleting instance groups and launch templates](#deleting-instance-groups-and-launch-templates) |

**Verified on a real host.** Security-group enforcement (allow and deny on live packets, status through a `machina-bpfd`
restart), run-instances, instance-type change, volume attach with live I/O limits and delete-on-termination, pinned DHCP
addresses, alarm-driven group scaling, group and template delete, private-image launch checks, the EC2 endpoint through boto3,
and the user-data path with a cirros guest. **Also verified live:** the NAT gateway, the metadata service, project-scoped keys and subnet delete. Elastic IPs and load balancer health checks are verified too, after two bugs the live runs found and fixed; only the health check's *recovery* back to healthy is still unit-tested only. See [the claims ledger](claims.md).

## Tags and ids
- `GET|PUT|DELETE /api/v1/tags/{resource_type}/{id}` stores key/value tags (≤ 50 per resource, keys ≤ 128, values ≤ 256).
  `GET /api/v1/tags?key=&value=` lists resources by tag. `GET /api/v1/vms?tag_key=&tag_value=` filters instances.
- Every taggable resource has an EC2-style id (`i-`, `vol-`, `snap-`, `sg-`, `key-`, `ami-`, `eni-`, `vpc-`, `subnet-`,
  `asg-`, `lt-` + 17 hex characters), derived from its UUID, so it never changes. `GET /api/v1/ids/{ec2_id}` resolves one;
  routes that take an instance id also accept the EC2 id.

## Instance types
`POST /api/v1/vms/{id}/change-type {flavor_id}` shuts the instance down cleanly, sets vCPUs and memory to the flavor's,
starts it again if it was running, and records `vms.flavor_id`. It is a task with events. The disk size is not changed.
Refused: a flavor with no usable size, a type the instance already has, an instance managed by an instance group (change
the launch template instead), and a change that would exceed the project's vCPU/memory quota.

## User data
`cloud_init_user_data` (≤ 16 KB) on instance create and launch templates is written verbatim as the NoCloud `user-data`
in the seed ISO (merged with the user/ssh key vendor data) and is never logged. There is no 169.254.169.254 metadata
service yet; the seed ISO is the delivery path.

## Security groups
Groups are **advisory** (`mode: audit`, the default for every existing group) until you switch them:
`PUT /api/v1/security-groups/{id}/mode {"mode":"enforce"}`.

- Attach to instances with `PUT|DELETE /api/v1/vms/{id}/security-groups/{sg_id}`; list with `GET .../security-groups`.
- Rules: ingress/egress, protocol tcp/udp/icmp/icmpv6/all, port or range, peer = CIDR **or** another group
  (`remote_sg_id`: every instance carrying that group). New groups get an allow-all egress rule, as in EC2.
- An enforcing instance gets one managed policy `sg-<instance name>` on the VM edge. Rules of all its groups are unioned
  (allow-only). Both directions are default-deny, so an enforcing group with no ingress rules accepts nothing, and one
  with no egress rule sends nothing. Replies to allowed connections pass (connection tracking).
- `enforcement.state` is what the hosts say, not what was requested: `advisory` (audit mode), `pending` (no instance
  attached), `enforced`, `auditing` (the host's VM edge is not enforcing), `failed` (a host did not answer).
- Enforcement is lease-gated and fails open: the controller re-asserts enforce mode and a fresh lease every 30 s
  (`MACHINA_BPF_ENFORCE_LEASE_SECS`); if the controller or `machina-bpfd` stops, the host drops back to observe and the
  state shows `auditing` until the next tick. Enforce mode is per host, so any other VM network policy on a host with an
  enforcing group is enforced too.
- Refused: enforcing while an attached instance has no known address (traffic could not be told from spoofed traffic).
- Dry run: `GET /api/v1/security-groups/{id}/enforce-preview` shows, per attached instance, how many inbound and outbound
  rules it would end up with and warns about the lockouts that matter (no inbound rule, no outbound rule, no TCP 22, a rule
  that opens every port to everyone). The Enforce button shows it before the confirmation click. It warns; it does not
  block, and it cannot see the controller's or the console's own network paths: allow what you need explicitly.
- Not covered yet: groups per network interface (they attach per instance).

## Volumes: delete on termination and I/O limits
- `delete_on_termination` (create body, attach body, or `PUT /api/v1/volumes/{id}/delete-on-termination {"value":true}`):
  when the instance is deleted, flagged volumes attached to it are deleted too. If a backend delete fails the volume is
  kept (detached) and the failure is logged; the instance delete still succeeds.
- `PUT /api/v1/volumes/{id}/iotune {read_iops, write_iops, read_bps, write_bps}`: `0` removes a limit, an omitted field is
  unchanged. On an attached volume the limits are applied to the disk through the agent (`virsh blkdeviotune`, live and
  persistent); the values are stored either way. They are not re-applied on a later attach yet.

## Key pairs by name
Instance create (from template, ISO or image) accepts `key_name`: the saved key pair's public key is injected through
cloud-init, like EC2's `KeyName`. Giving both `key_name` and `cloud_init_ssh_pubkey` is rejected, and an unknown name fails
with `keypair_not_found`. Machina still stores public keys only: there is no create-and-return-the-private-key call, so
generate keys with `ssh-keygen` and register the public half.

## Run several instances (RunInstances)
`POST /api/v1/vms/run-instances` takes the same body as `from-template` plus `count` (1–20) and optional `min_count`
(default `count`). With `count > 1` the machines are named `name-1 … name-N`. They are created one after another; if
fewer than `min_count` could be created the call returns 409 `run_instances_min_count` and lists the ones already
created, which are left in place (no rollback). Quota and placement are checked per instance, so a project quota that
fits only some of them yields a partial result when `min_count` allows it.

## Ids and tag filters beyond instances
Volumes, security groups and key pairs now return `ec2_id` (`vol-…`, `sg-…`, `key-…`). `GET /api/v1/volumes?tag_key=&tag_value=`
filters volumes by tag like the instance list does.

## Image visibility and sharing (AMI-style)
Images (templates) are `public` by default, as before. `PUT /api/v1/templates/{name}/{version}/visibility {"visibility":"private"}`
makes one visible only to its owning project and the projects it is shared with
(`PUT|DELETE .../shares/{project}`, `GET .../shares`). `GET /api/v1/templates?project=<name>` returns what that project
may see; without `project` you get everything (the operator view). Images return `ec2_id` (`ami-…`).

Launching is checked too: creating an instance from a private image fails with 403 `image_not_shared` unless the instance's
project owns the image or it was shared with that project. The project is the instance's own (`metadata.project`, default
`default`), so a caller who may create instances in a project can launch that project's shared images.

## Volume from snapshot
`POST /api/v1/volume-snapshots/{id}/create-volume {"name"}` creates a new volume (same size, class and project as the
source) cloned from the snapshot through Atlas. Snapshots exist only for Atlas-backed volumes, so this needs
`ATLAS_ENABLED=1`; local-pool volumes have no snapshots yet.

## Network interfaces (ENIs)
A port (`POST /api/v1/ports`) on a cloud subnet now behaves like an ENI: it reserves a managed address of the subnet
(`private_ip` in the request, or the first free one), returns `private_ip`, `subnet_id`, `mac_address` and an `eni-` id, and
releases the address when the port is deleted. A requested address must be in the managed (lower-half) range and free,
otherwise 409.

When the port is attached to an instance, its reserved address is pinned as a DHCP host entry (MAC to address) on the
subnet's libvirt network, so a guest that uses DHCP is handed exactly that address (`dhcp_pinned` on the port says whether it
worked; a failure is logged and the port still exists). Deleting the port unpins it.

**Limits:** a guest with a static configuration must still be set up with the address by hand. There are no secondary
addresses and a port still carries one security group; per-interface groups are not enforced (groups attach to instances).

## Metric statistics
`GET /api/v1/metrics/statistics?subject=<vm name, i- id or uuid>&metric=<metric>&period=300&statistics=Average,Maximum&start=&end=` buckets
the stored samples (`metric_samples`) into epoch-aligned windows of `period` seconds (a multiple of 60, 60–86400) and returns
`Average`, `Minimum`, `Maximum`, `Sum` and `SampleCount` per window. The default range is the last hour; the limit is 15
days and 1440 datapoints per call. Retention is whatever the sampler keeps; there are no custom dimensions or units yet.

## Alarms
`POST /api/v1/alarms` creates a CloudWatch-style alarm over the stored metric samples: `subject` (a VM name, its `i-` id or UUID, or
`group:<group id, 32 hex>` for every member of an instance group; the sampler records `cpu_percent`, `mem_ratio`, `disk_iops` and
`net_bytes` per machine every minute), `metric`, `statistic`, `period_secs` (60–3600, multiples
of 60), `evaluation_periods` (1–10), `comparator` (`gt|gte|lt|lte`) and `threshold`. It is `OK`, `ALARM` or
`INSUFFICIENT_DATA` (fewer than `evaluation_periods` windows with data) and re-evaluated every minute on the leader; state
changes are events and carry a reason.

With `action: "scale_group"`, `group_id` and a non-zero `step` (±10) the alarm changes the group's desired size on entering
`ALARM`, and again every `cooldown_secs` while it stays there. The result is clamped to the group's min/max and uses the same
compare-and-swap as the group reconciler, so a concurrent policy edit wins. Paused groups are left alone. Re-enabling an alarm
(`PUT /api/v1/alarms/{id}/enabled`) resets it to `INSUFFICIENT_DATA`.

**Limits:** no notification actions (use alert rules and webhooks for those), no alarm history table beyond events, and
`INSUFFICIENT_DATA` does not trigger anything.

## Deleting instance groups and launch templates
`DELETE /api/v1/cloud/instance-groups/{id}` removes a group once none of its members is running (scale it to min 0 and desired 0,
let them stop, then delete). The stopped instances and their disks are kept; they just stop being managed. Alarms that scaled
the group lose their action. A refused delete leaves the group unpaused and unchanged.
`DELETE /api/v1/cloud/launch-templates/{id}` removes a template that no group uses (409 otherwise).
Subnet and VPC delete with libvirt network teardown are still not implemented.

## Elastic IPs
An admin defines a pool of public addresses a host holds (`POST /api/v1/elastic-ip-pools {name, cidr (/16–/32), host_id,
interface}`; the interface name is at most 11 characters). Operators then use:

- `POST /api/v1/elastic-ips {pool?}` allocates the first free address (`eipalloc-…` id), `GET /api/v1/elastic-ips` lists them;
- `POST /api/v1/elastic-ips/{id|address}/associate {vm_id}` maps it 1:1 to an instance on the pool's host (one elastic IP per
  instance; the instance needs a known address), `…/disassociate` frees it, `DELETE …` releases an unassociated address;
- the response says whether the host confirmed the mapping (`applied`, `apply_error`).

On the host (the agent's `eip.sync`): the address is held as a `/32` alias `<iface>:eip` on the interface, traffic to it is
DNATed to the instance (from outside and from the host itself), the instance's traffic to non-private destinations is SNATed
to it, and the DNATed connections are accepted ahead of libvirt's own forward rules. Three iptables chains of our own
(`MACHINA_EIP_DNAT`, `MACHINA_EIP_SNAT`, `MACHINA_EIP_FWD`) are rebuilt on every push (every 30 s and on each change), so a
libvirt restart or a manual flush heals itself.

**Limits:** host-local (the instance must be on the host that holds the pool; no failover with the instance), IPv4 only, the
address must be routed to the host by your network (the agent announces it with a gratuitous ARP when `arping` exists), and
there is no NAT gateway for private subnets yet. Security groups apply to the instance as before.

## NAT gateway for private subnets
`PUT /api/v1/cloud/subnets/{id}/nat {"enabled": true}` lets the instances of a ready subnet reach the outside through their
host: traffic leaving the subnet is masqueraded behind the host's uplink (the interface of its default route), and the
subnet's traffic and its replies are accepted ahead of libvirt's forward rules. Two iptables chains of our own
(`MACHINA_NAT`, `MACHINA_NAT_FWD`) are rebuilt every 30 s and on each change; switching the last subnet off clears them.
The response says whether the host confirmed it (`applied`, `apply_error`).

**Limits:** IPv4, host-local. Cloud subnets are libvirt isolated networks that do not advertise a default gateway, so an
instance still needs a default route via the subnet's gateway address (the `.1` of the subnet) set by its own configuration
(cloud-init network config or user-data); the NAT gateway makes that route work, it does not install it.
## Load balancer health checks
`PUT /api/v1/load-balancers/{id}/health-check {protocol: "tcp"|"http"|"none", port?, path?, interval_secs (5–300), timeout_secs,
healthy_threshold, unhealthy_threshold}` makes the owning host's agent probe every member (a TCP connect, or an HTTP GET that
must answer 2xx or 3xx) from the host, which is the only place that can reach the guests' private addresses. A member shows
`health: unknown | healthy | unhealthy` with the last probe's detail. It turns `unhealthy` after `unhealthy_threshold`
consecutive failures and is taken out of the rule set; it comes back after `healthy_threshold` consecutive successes. A
member that was never probed stays in rotation, as does everything when the check is `none` (the default). Changing the check
resets the members to `unknown`. State changes are events (`lb.health`).

This is the existing kernel round-robin balancer plus health checks, not target groups: one check per balancer, L4 only.
## Deleting subnets
`DELETE /api/v1/cloud/subnets/{id}` tears down the subnet's libvirt network on its host (through the agent) and removes the
subnet. It is refused (409) while the subnet still has reserved addresses, network interfaces, instance groups or instances,
and it fails without deleting anything if the host cannot be reached or libvirt refuses. A network that is already gone is
not an error. `DELETE /api/v1/cloud/vpcs/{id}` works once the VPC has no subnets and no peerings.
## Instance metadata service
Guests can read their own metadata at `http://169.254.169.254/latest/meta-data/` (also under dated versions such as
`/2009-04-04/`, which cirros and other EC2 datasources use): `instance-id`, `hostname`, `local-ipv4`, `instance-type`,
`ami-id`, `placement/availability-zone`, `public-keys/0/openssh-key`, plus `/latest/user-data` and
`/latest/dynamic/instance-identity/document`.

How it works: the controller pushes one entry per instance to its host's agent every 30 s; the agent serves them on port
8169 and an nft rule (`table ip machina_imds`) redirects `169.254.169.254:80` to it. The answer is chosen by the request's
**source address**, so a guest sees only its own entry and an unknown address gets 404. Disable with `MACHINA_IMDS=0` (agent
and controller); `MACHINA_IMDS_PORT` changes the port.

**Limits:** it needs an address the controller knows for the guest (DHCP lease or guest agent). Guests need a route to the
link-local address, which the default NAT network provides through its gateway; isolated cloud subnets do not advertise a
gateway, so guests there need a route to `169.254.169.254` via the subnet's gateway address (not automatic yet). There is no
IMDSv2 token and no hop limit: as on EC2, anything running in the guest, including a vulnerable web application, can read
the user-data, so keep long-lived secrets out of it. Tags are not exposed yet.
## Project-scoped API keys
`POST /api/v1/api-keys` accepts `projects: ["lab", "web"]`. A scoped key (operator or viewer, never admin; its name must be
unique) can use only:
- the cloud APIs (`/api/v1/cloud/...`) of those projects, with its own role capping what it may write; and
- the instance routes (`/api/v1/vms/{id}/...`) of machines in those projects, and the machine list filtered to them.

Everything else (hosts, storage, security, other projects) answers 403 `key_scope_forbidden`, and creating machines through
the plain instance API is refused: scoped keys launch through launch templates and instance groups. The scope is enforced
whether or not `MACHINA_PROJECT_RBAC` is on. Rotating a key keeps its scope. Unscoped keys behave as before.
