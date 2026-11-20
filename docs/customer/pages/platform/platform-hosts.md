# Hosts

## Purpose

Every hypervisor enrolled with the controller: online/offline state, agent heartbeat, capacity and the VMs it runs. This is where you start any host-level task in the fleet.

## When to use it

- Check whether a host is online and its agent heartbeat is current
- Find hosts that went offline (`?filter=offline`)
- Re-sync inventory after changes made outside Machina
- Validate a newly joined host before you place VMs on it

## How to get there

- Route: `/platform/hosts`
- Nav: **Platform → Hosts** (or spotlight / Finder search)

## What you can do

1. The list shows each managed host with status and last heartbeat. Use search, or open `?filter=offline` to see only offline hosts.
2. **Sync all** asks every agent to refresh its VM, storage and network inventory.
3. Select a host and **Open host** for its detail page (VMs, capacity, maintenance mode, agent version).
4. **Validate** runs host join validation (agent reachable, libvirt connected, versions compatible) and shows the result under **Validation**.
5. **Open in Machine Finder** jumps to the fleet VM list filtered to that host.

## If something is wrong

- **No hosts:** enroll one from [Host Enrollment](platform-enroll.md).
- **Host offline:** on the host, check `systemctl status machina-agent` and that it can reach the controller on port 5093 (and the controller can reach the agent on 50051).
- A host whose heartbeat is older than 90 seconds is treated as failed by HA; see [High Availability](platform-ha.md).

## Related pages

- [Host Enrollment](platform-enroll.md)
- [Machine Finder](platform-vms.md)
- [High Availability](platform-ha.md)
- [Upgrade Matrix](platform-upgrade.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
