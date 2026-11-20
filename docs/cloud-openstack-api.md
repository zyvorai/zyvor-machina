# OpenStack-compatible API

Machina exposes a Keystone / Nova / Glance / Cinder / Neutron **compatibility façade** on the
controller (`:5093`) so `python-openstackclient` and Terraform's `openstack` provider can drive
Fleet Cloud without speaking Machina's native REST. Each request is reshaped onto the existing
handlers; there is no separate Nova or Neutron control plane.

Status: **unit-tested** (token issue, flavors) and **live-checked** on lab host `212.8.248.187`
(2026-11-12): OSC `token issue`, `catalog list`, `image list`; curl list ops for Nova/Cinder/Neutron.
See claims **C74**. Full OSC server CRUD and Terraform OpenStack provider matrix are not yet run.

## Endpoints

| Service | Base path | Covered ops |
|---------|-----------|-------------|
| Keystone v3 | `/v3` | `POST /v3/auth/tokens`, catalog, projects, users, version discovery |
| Nova v2.1 | `/v2.1` | flavors CRUD, servers CRUD/power (`os-start`/`os-stop`/`reboot`/`pause`/`unpause`/`suspend`/`resume`/`resize`/`createImage`), remote-consoles, os-interface, keypairs, server groups (anti-affinity) |
| Glance v2 | `/v2` | images list/show/create/delete ← Fleet Cloud templates |
| Cinder v3 | `/v3` (+ `/{project_id}/…`) | volumes CRUD, attach/detach/`os-extend`, create-from-snapshot, snapshots |
| Neutron v2.0 | `/v2.0` | networks (= VPCs), subnets, ports CRUD, security groups + rules, floating IPs, QoS policies |
| Octavia L4 | `/v2.0/lbaas` | loadbalancers + pool members ← native iptables LB (`machina_host_id` / `machina_vm_id`) |

Public auth and version routes are rate-limited with `/ec2` and login. Token-gated service routes sit
beside the rest of the controller API.

Set `MACHINA_PUBLIC_URL` so the Keystone catalog advertises the right host (default
`http://127.0.0.1:5093`).

## Auth

```bash
export OS_AUTH_URL=http://HOST:5093/v3   # or https://… when TLS terminates in front
export OS_USERNAME=admin
export OS_PASSWORD=…                     # controller local user (MACHINA_ADMIN_PASSWORD on a fresh platform install)
export OS_PROJECT_NAME=default
export OS_USER_DOMAIN_NAME=Default
export OS_PROJECT_DOMAIN_NAME=Default
export OS_IDENTITY_API_VERSION=3
# self-signed / plain-http lab:
export OS_INSECURE=true
```

`POST /v3/auth/tokens` accepts:

- **password** — local controller users (`auth::authenticate`)
- **application_credential** — secret is a Machina API key (`machina_…`)

Tokens are opaque rows in `os_tokens` (migration **092**, 24 h TTL) and are sent as `X-Auth-Token`
on later calls. A successful issue returns `201` with `X-Subject-Token`.

## Smoke

```bash
openstack token issue
openstack catalog list
openstack flavor list
openstack server list
openstack image list
openstack volume list
openstack network list
openstack security group list
openstack floating ip list
```

Live helper (daemon cookie proxy or direct controller JWT):

```bash
export MACHINA_BASE_URL=https://127.0.0.1:15092 MACHINA_USER=sus MACHINA_PASS=max
./scripts/openstack/mock-import-smoke.sh
# or:
export MACHINA_CONTROLLER_URL=http://127.0.0.1:15093 MACHINA_ADMIN_PASSWORD=…
./scripts/openstack/mock-import-smoke.sh
```

## First-party importer

Separate from the wire façade: connect to a **source** OpenStack, sync inventory, import Glance
images (and Cinder volumes via `os-volume_upload_image`) as Fleet Cloud templates.

| Surface | Path |
|---------|------|
| REST | `/api/v1/openstack/connections`, `…/inventory`, `…/imports` (`api/openstack_import.rs`) |
| Engine | `engine/openstack/` — Keystone client, inventory, import/stage, in-process mock |
| UI | Platform → Migration → **OpenStack** |
| Schema | migration **093** (`openstack_connections`, `openstack_inventory`, `openstack_import_jobs`) |
| Smoke | unit: `cargo test -p machina-controller openstack`; live list: `scripts/openstack/mock-import-smoke.sh` |

Credentials are stored with `crypto::store_api_key`. Land path: curl stage on host with
`X-Auth-Token` → INSERT `templates` (`source_disk`). Volume imports call Cinder
`os-volume_upload_image`, wait for Glance `active`, then download. **Not yet run** against a
production OpenStack cloud (claim **C75**).

## VM QoS (Neutron-ish)

Controller QoS policy objects push VM-edge rate limits (`egress_mbps` / `ingress_mbps` / `pps`)
into bpfd on bind (forced `vm_netpol` reconcile). Neutron façade:
`/v2.0/qos/policies` (+ bandwidth_limit_rules).

```
GET|POST /api/v1/vm-qos/policies
DELETE   /api/v1/vm-qos/policies/{id}
POST     /api/v1/vm-qos/policies/{id}/bind
```

Migration **094**. Live create exercised on the lab host (claim **C76**).

## Limits

- Not a drop-in for every microversion or extension. Unsupported actions return OpenStack JSON
  errors (`Not Implemented` / `Bad Request`).
- Neutron networks map to Fleet Cloud VPCs (need an online host; optional `machina:host_id` /
  `machina:cidr` on create). No OVN/VXLAN tenant overlay, provider/flat uplink networks, full
  Neutron BGP/L3 HA, or amphora Octavia.
- Server groups only support anti-affinity (VM tag `anti-affinity:<name>`).
- Glance create registers an existing host disk path (`machina_source_disk`); it does not upload
  image bytes through the Glance HTTP body.
- Floating IP associate uses `machina_vm_id` (or a VM UUID in `port_id`).
- Hyperscale Keystone federation for thousands of tenants is out of scope.

Native REST and the EC2 Query API remain first-class. Service map and cutover steps:
[from-openstack.md](migration/from-openstack.md). Fabric peering delete / IGW / NAT / transit:
[cloud-fabric.md](cloud-fabric.md).
