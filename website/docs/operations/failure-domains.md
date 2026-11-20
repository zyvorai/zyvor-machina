---
sidebar_position: 6
title: Failure domains and rack loss
description: Label hosts with a site and rack, spread anti-affinity groups across racks, and check whether HA could restart a lost rack.
---

# Failure domains and rack loss

A **failure domain** is the set of hosts that fail together: a rack on one power feed or top-of-rack switch, or a room. Machina
takes it from two labels every host already has, **site** and **rack**. A host with no rack label is in no domain.

Set the labels when a host joins (enrollment tokens carry a site and rack) or later:

```bash
curl -sk -X PATCH https://HOST:5093/api/v1/hosts/$HOST_ID \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"site":"dc1","rack":"r12"}'
```

## What uses them

- **Placement.** A VM tagged `anti-affinity:<group>` is steered away from any rack that already runs a peer, not only from
  the same host. It is a preference with the same fallback as host anti-affinity: if every rack would violate it, the VM is still
  placed. DRS rebalancing keeps its host-level rule and does not use racks.
- **HA recovery.** When HA restarts a VM it tries hosts in racks that hold no member of the VM's anti-affinity group first, which
  also puts it outside the rack it just lost if that rack still has online hosts. It is a preference: a rack with a peer is used
  when nothing else has room.
- **Readiness report.** `GET /api/v1/failure-domains` answers: if this rack went dark, could HA restart what ran in it on the
  hosts that remain?

```json
{
  "rack_loss_tolerant": false,
  "domains": [{
    "domain": "dc1/r12", "hosts": 4, "ha_vms": 18,
    "ha_memory_to_recover_mib": 262144,
    "headroom_outside_mib": 196608,
    "spare_after_loss_mib": -65536,
    "survivable": false
  }],
  "findings": ["Losing dc1/r12 leaves 262144 MiB of HA memory without room (short by 65536 MiB)."]
}
```

The check counts memory only (the hard limit HA also uses) on hosts that are online, out of maintenance and schedulable. Cordoned
hosts do not count as spare room.

## Witness: do not act on silence

If the controller loses its own network, every host looks dead. Without a tie-breaker it would fence them all and restart their
VMs elsewhere. A **witness** is any HTTP(S) endpoint in a third failure domain (a small static server, an object-store URL, a
monitoring beacon). When one or more witnesses are configured, HA fences hosts and recovers VMs **only while the controller can
reach them**.

```bash
# controller environment
MACHINA_WITNESS_URLS=https://witness.site-c.example/ok,https://10.9.0.5:8443/ok   # up to 5
MACHINA_WITNESS_MODE=any            # any (default): one reachable is enough; majority: more than half
```

Any HTTP answer below 500 counts as reachable. Checks are cached for 15 seconds. When the witnesses become unreachable HA pauses and
records `ha.witness_unreachable` once; `ha.witness_restored` follows when they return. `GET /api/v1/ha/witness` and the Failure
Domains page show the current state (URLs appear as host and port only). With no URLs set nothing changes.

This is an isolation guard, not distributed consensus: it stops a cut-off controller from acting, and it does not arbitrate between
two controllers. A witness outage pauses HA, so use a reliable one.

## What it does not do yet

- No quorum between controllers: the witness only stops an isolated controller from acting. Losing the rack that holds the
  controller is a separate question. See [Controller HA](https://github.com/zyvorai/zyvor-machina/blob/main/docs/controller-ha.md).
- Storage and network survival are not checked. Pair this with replicated storage that has its own failure domains.
- The witness pause and resume were run on a real controller with a real witness (2026-10-22, claim C65). That was a witness outage on one host, not a network partition. Rack placement and the rack-loss report have not been run on real racks (C57).

Run a drill on hardware you can afford to lose before relying on it: power off one rack's hosts and watch `ha_events`.
