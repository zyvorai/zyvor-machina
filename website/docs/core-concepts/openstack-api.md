---
sidebar_position: 10
title: OpenStack-compatible API
description: Keystone, Nova, Glance, Cinder and Neutron façades on the controller, plus the first-party importer.
---

# OpenStack-compatible API

The controller answers a **subset** of the OpenStack Identity / Compute / Image / Volume / Network HTTP APIs so
`python-openstackclient` and Terraform's `openstack` provider can drive Fleet Cloud. Every call is reshaped onto Machina's
existing handlers — there is no separate Nova or Neutron process.

:::note Verification status
Unit-tested (token issue, flavors, mock importer). Live on lab host 2026-11-12: OSC `token issue`, `catalog list`,
`image list`; curl list ops; importer connection + VM QoS create. **Not yet:** full OSC server CRUD, Terraform OpenStack
provider matrix, import from a production OpenStack cloud. Claims **C74–C76**.
:::

## Endpoints (controller `:5093`)

| Service | Base | Covered |
| --- | --- | --- |
| Keystone v3 | `/v3` | Token issue/validate, catalog, projects, users |
| Nova v2.1 | `/v2.1` | Flavors, servers CRUD/power/resize/createImage, remote-consoles, os-interface, keypairs, server groups |
| Glance v2 | `/v2` | Image list/show/create/delete ← templates |
| Cinder v3 | `/v3` | Volumes, attach/detach/extend, create-from-snapshot, snapshots |
| Neutron v2.0 | `/v2.0` | Networks (= VPCs), subnets, ports CRUD, security groups, floating IPs, QoS policies |
| Octavia L4 | `/v2.0/lbaas` | Load balancers + members ← native iptables LB |

Set `MACHINA_PUBLIC_URL` so the catalog URLs match the host clients reach (default `http://127.0.0.1:5093`).

## Auth

```bash
export OS_AUTH_URL=http://127.0.0.1:15093/v3   # SSH tunnel to controller :5093
export OS_USERNAME=admin
export OS_PASSWORD=…                          # MACHINA_ADMIN_PASSWORD on a fresh platform install
export OS_PROJECT_NAME=default
export OS_USER_DOMAIN_NAME=Default
export OS_PROJECT_DOMAIN_NAME=Default
export OS_IDENTITY_API_VERSION=3
export OS_INSECURE=true

openstack token issue
openstack catalog list
openstack image list
```

Password auth uses controller local users. `application_credential` secrets are Machina API keys (`machina_…`).
Opaque tokens live in `os_tokens` (~24 h) and are sent as `X-Auth-Token`.

## First-party importer

Connect Machina to a **source** OpenStack (Keystone URL + credentials), sync Glance/Nova/Cinder inventory, and import
Glance images (or Cinder volumes via upload-to-image) as Fleet Cloud templates.

- **UI:** Platform → Migration → **OpenStack**
- **API:** `/api/v1/openstack/connections`, `…/inventory`, `…/imports` (through the daemon proxy or `:5093`)
- **Land path:** host stage with `X-Auth-Token` → template with `source_disk`
- **Smoke:** `scripts/openstack/mock-import-smoke.sh` (list endpoints); unit mock download in
  `cargo test -p machina-controller openstack`

## VM QoS

Neutron-style rate limits bind into the VM-edge datapath (`egress_mbps` / `ingress_mbps` / `pps`) and also appear as
`/v2.0/qos/policies` on the façade.

`GET|POST /api/v1/vm-qos/policies`, `DELETE …/{id}`, `POST …/{id}/bind`.

## Limits

- Smoke set only — unsupported actions return OpenStack JSON errors, never silent success.
- No OVN/VXLAN tenant overlay, provider/flat uplink networks, full Neutron BGP/L3 HA, Heat HOT, Magnum/Trove/Manila, or amphora Octavia.
- Glance create registers a host disk path; it does not stream image bytes through the Glance HTTP body.
- Server groups: anti-affinity only.

Related: [Replacing OpenStack](../getting-started/from-openstack.md), [EC2-compatible API](ec2-api.md),
[VPC forwarding](vpc-forwarding.md), [Fleet Cloud](fleet-cloud.md).
