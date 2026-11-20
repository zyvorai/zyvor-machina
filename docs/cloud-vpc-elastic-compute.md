# VPC foundations and elastic compute

Machina exposes a project-owned cloud API at `/api/v1/cloud`. The first backend
creates **host-local isolated IPv4 libvirt subnets**, immutable VM launch
templates, and reconciled instance groups. Open Fleet Cloud → More → VPCs &
elastic compute (`/fleet-cloud/vpcs`) to create and inspect these resources.

## What runs today

| Resource | Behavior |
|---|---|
| VPC | Project and host ownership, RFC1918 CIDR, host-wide overlap checks |
| Subnet | CIDR containment and overlap checks; durable provisioning task; isolated libvirt network with DHCP |
| IPAM | Transactional allocation; caller request keys are idempotent; release supported |
| Routes | Validated local, blackhole, internet-gateway or accepted-peering routes; peering, gateway and blackhole routes are installed in the host datapath (see [cloud-fabric.md](cloud-fabric.md)) |
| Peering | Explicit request and target-project acceptance; accepted peerings carry traffic once a route selects them, over the `machina-wg` overlay |
| Launch template | Immutable VM spec; template lists omit the stored spec; plaintext guest passwords/public consoles rejected |
| Instance group | Desired count, min/max, pause, optional CPU target and cooldown; leader loop creates VMs and reuses the existing quota/policy checks |
| Scale-in | Stop and retain VM/disks; increasing desired resumes retained slots |

This is not AWS/Azure/GCP API compatibility or a complete hyperscaler clone.
There are no transit/VPN gateways, private endpoints, managed databases or IPv6
forwarding in this release. Peering, internet-gateway and blackhole routes and
network ACLs are enforced on the hosts ([cloud-fabric.md](cloud-fabric.md));
cross-host peering has been run on two hosts with test traffic (claim C52). Existing Fleet Cloud security groups remain advisory; this change does
not claim to activate them. Use the separate VM network policy service where
appropriate, accounting for its enforcement lease and preview maturity.

A VPC currently groups independent isolated subnets on **one host**. VMs in the
same subnet can communicate; separate subnets have no inter-subnet forwarding.
The subnet gateway/DNS address is served by libvirt/dnsmasq. The existing host
network firewall and libvirt isolated-network semantics are prerequisites.
Cross-host migration is rejected at the API and worker boundaries; DRS skips
these instances. HA is rejected in cloud launch templates and cross-host recovery
is blocked by the worker. There is no automatic load-balancer membership
or health-based replacement in v1.

## Authorization and ownership

Cloud API access always checks the owning project's enabled state and membership,
regardless of the legacy `MACHINA_PROJECT_RBAC` setting. Global admins can access
enabled projects. Other users need explicit membership; mutations also need a
global operator role and an operator/admin project role. Unknown or unscoped
service identities are denied unless globally admin. List endpoints are scoped
by project. VM creation for a group replaces the template's project and network
with the group's authorized project/subnet and pins placement to its host.

Projects/hosts/templates/subnets with dependencies cannot be deleted through
foreign-key constraints. VPC deletion explicitly rejects subnet/peering
references. Subnet teardown and group deletion are intentionally not exposed:
stop/pause groups, retain data, and use an operator-controlled maintenance flow.
Retained stopped instances continue consuming project VM/storage quota.

## Things to know before you rely on it

- **The page sets capacity by hand.** `/fleet-cloud/vpcs` creates groups with a fixed `min 0 / max 10` policy and no CPU
  target. CPU autoscaling (`target_cpu`, `cooldown_secs`, up to `max` 100) is configured through the API.
- **Scoped API keys are not supported.** Cloud APIs check project membership for the signed-in *user*; an API key is not a
  member of any project, so non-admin keys receive `403`. Admin-role keys bypass project scoping entirely. Use a user
  session, or an admin key for automation, until scoped keys exist.
- **A host that owns a VPC cannot be removed** (`409 host_has_vpcs`): there is no VPC deletion yet, and removing the host would
  orphan its libvirt networks.
- **Nothing deletes a network yet.** Subnets, templates, groups and peerings have no delete routes in this release; a
  provisioned `mc-<uuid>` libvirt network stays until removed by hand (`virsh net-destroy` / `net-undefine`).
- **A provisioned network is re-checked on retry.** If a network with the subnet's UUID already exists but its subnet changed
  or it gained a forwarding mode, provisioning fails with a clear `drifted` error instead of reporting success.
- **A stale leader cannot create instances.** The reconcile loop stops as soon as this controller loses leadership or the
  leadership epoch changes.
- **Run the smoke script as root, or point it at the system libvirt** (`virsh -c qemu:///system`): with a plain user it talks
  to a per-user session that has no bridges.

## API walkthrough

Use your existing bearer token with the controller or the daemon's authenticated
controller proxy. `$BASE` below is the controller base URL; for the daemon use
`https://HOST:5092/api/v1/platform/controller`. IDs come from the project registry
and host inventory. Requests below are JSON bodies, not shell commands with
embedded credentials.

1. `POST $BASE/api/v1/cloud/projects/{project_id}/vpcs`

```json
{"name":"production","cidr":"10.20.0.0/16","host_id":"HOST_UUID"}
```

2. `POST $BASE/api/v1/cloud/vpcs/{vpc_id}/subnets`

```json
{"name":"applications","cidr":"10.20.1.0/24"}
```

The response contains `id`, `network_id`, `task_id`, and `status: pending`.
Poll `GET /api/v1/cloud/vpcs/{vpc_id}/subnets` and the existing
`GET /api/v1/tasks/{task_id}`. `ready` is written only after the agent confirms
network creation/start/autostart. If provisioning fails, upgrade/check the
agent, inspect `last_error`, and `POST /api/v1/cloud/subnets/{id}/retry`.
Old agents reject `cloud-isolated` instead of silently producing a bridge.
Upgrade controller and agents together.

3. Optional managed address reservation:
`POST /api/v1/cloud/subnets/{subnet_id}/addresses`

```json
{"request_key":"application-01"}
```

Requests using the same key return the same reservation. `.0` through `.3` are
reserved. The lower half (`.4` through `.127` in a /24) is managed IPAM; the
upper half (`.128` through `.254`) is DHCP. These ranges never overlap. The
broadcast address is not allocated. An IPAM reservation **does not configure a
VM**: set it inside the guest and release it only after removing that address
from the guest. Group instances use DHCP. Exhaustion returns 409.

4. `POST /api/v1/cloud/projects/{project_id}/launch-templates`

```json
{
  "name":"web-v1",
  "vm":{
    "api_version":"virt.zyvor.dev/v1",
    "kind":"VirtualMachine",
    "metadata":{"name":"base"},
    "spec":{
      "cpu":{"sockets":1,"cores":2},
      "memory":"2Gi",
      "storage":[{"name":"root","size":"10Gi","class":"local","source":"/var/lib/machina/images/base.qcow2"}],
      "cloud_init":{"user":"ubuntu","ssh_pubkey":"ssh-ed25519 YOUR_PUBLIC_KEY"}
    }
  }
}
```

The source is an example: provide a bootable image or existing template reference
that the owning host can access. Empty disks alone do not produce a bootable OS.
Template names are unique per project; create a new name for a new version.
Template names, project names and VM specs use the existing Machina validators.

5. `POST /api/v1/cloud/projects/{project_id}/instance-groups`

```json
{
  "name":"web",
  "template_id":"TEMPLATE_UUID",
  "subnet_id":"READY_SUBNET_UUID",
  "policy":{"min":1,"max":4,"desired":2,"target_cpu":60,"cooldown_secs":300}
}
```

Inspect `GET /api/v1/cloud/instance-groups/{id}` for member slots and observed VM
states. `PATCH` accepts a complete `policy` and a `paused` boolean. Bounds must
satisfy `0 <= min <= desired <= max <= 100`; cooldown is 30–86400 seconds. Set
`target_cpu: null` for manual scaling. A paused group makes no further scaling
changes; the generic VM reconciler continues previously requested power changes.

CPU autoscaling requires every desired instance to be running and have a sample
newer than two minutes. Missing/stale/invalid samples hold the desired count.
CPU above target+10 adds one instance; below target-10 stops one, subject to
bounds and cooldown. No scale-from-zero based on CPU exists. Use manual desired
count to start a zero-sized group. VM creation errors are visible on the group;
fix image/host/quota errors before retrying on the next loop. The background loop
runs every 30 seconds, and the VM reconciler performs power convergence.

## Recovery and audit

Subnet resource and task rows commit in a single `BEGIN IMMEDIATE` transaction.
The leader republishes pending jobs after a bus outage; existing worker task
claims suppress duplicate execution. The agent uses subnet UUIDs for ownership
and deterministic libvirt/bridge names. It refuses an existing network with a
different UUID. It starts inactive owned networks on retry and never rewrites
an existing network. VPC and subnet CIDRs are immutable.

Instance names are deterministic per group/slot. If a process dies after VM
creation but before member bookkeeping, reconciliation adopts only a VM with
the exact generated spec in the same project. It never adopts another spec.
Existing VM creation provides transactional quota reservations, task dispatch,
and audit. Manual cloud mutations and autoscaling have audit records. Review
both group `last_error` and existing task/VM lifecycle errors; desired count is
not a claim that all instances booted successfully.

## Validation

```bash
cargo test -p machina-spec --locked
cargo test -p machina-controller --lib api::cloud::tests --locked
cd web && npm test && npm run build
```

Full controller builds require the sibling GuestKit checkout and Linux libvirt,
PAM, OpenSSL, libclang, and protobuf build packages, as documented in CI.
See [the hardware smoke script](../scripts/cloud-subnet-smoke.sh) for a disposable
isolated network check on a dedicated libvirt test host. This must be run before
production qualification. It does not prove cross-host forwarding (see `docs/claims.md` C52), HA, guest
boot, or eBPF security-group enforcement.
