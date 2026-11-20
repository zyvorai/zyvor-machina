# High Availability

## Purpose

Fleet high availability: which hosts are healthy, fence events, and HA restarts of VMs from failed hosts. The controller fences a failed host before it restarts that host's VMs elsewhere, so a VM never runs twice.

## When to use it

- Confirm the fleet is healthy and fencing is configured
- Review what happened after a host failure (fence and HA events)
- Fence a host by hand when you know it is broken

## How to get there

- Route: `/platform/ha`
- Nav: **Platform → High Availability** (or spotlight / Finder search)

## What you can do

1. **Host Fencing** lists hosts and their state. **Fence** on a host (with confirmation) isolates it; use it only when the host is really unhealthy.
2. **Fence Events** shows each fence attempt and its result.
3. **HA Events** shows VMs restarted on other hosts after a failure.

How automatic HA works:

- A host whose agent heartbeat is older than **90 s** is considered failed. The controller scans for failed hosts every **45 s**.
- Before recovering VMs it fences the host: IPMI power-off from the controller when the host has BMC details, otherwise the command in `MACHINA_FENCE_COMMAND`.
- If fencing fails, VMs are **not** restarted unless the operator opted in with `allow_unfenced`.
- Only one controller acts at a time when several run (leader lease).

## If something is wrong

- **"Enroll hosts to enable fencing":** no hosts yet; see [Host Enrollment](platform-enroll.md).
- **Fence fails:** check BMC address and credentials on the host (Bare Metal) or the fence command, and that the controller can reach the BMC network.
- Details for admins: `docs/controller-ha.md` in the repository.

## Related pages

- [Placement & HA](platform-placement.md)
- [Hosts](platform-hosts.md)
- [Maintenance](platform-maintenance.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
