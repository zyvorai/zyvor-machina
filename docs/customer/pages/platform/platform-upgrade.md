# Upgrade Matrix

## Purpose

Version compatibility between the controller and host agents (controller version, minimum and recommended agent) and a per-host **Upgrade Agent** action.

## When to use it

- Before or after upgrading the controller, to see which agents are behind
- Rolling agent upgrades, one host at a time

## How to get there

- Route: `/platform/upgrade`
- Nav: **Platform → Upgrade Matrix** (or spotlight / Finder search)

## What you can do

1. **Upgrade Matrix** shows the controller version, the minimum and the recommended agent version.
2. **Hosts** lists each host's agent version against that matrix.
3. Follow the **Upgrade Guide** for each host:
   1. Put the host into maintenance mode from [Hosts](platform-hosts.md).
   2. Click **Upgrade Agent**; this enqueues a `host.agent.upgrade` task.
   3. Watch it in [Tasks](platform-tasks.md).
   4. When the agent heartbeat is back, take the host out of maintenance.

## If something is wrong

- **No hosts enrolled:** enroll a host first.
- **Upgrade task fails:** check the task message in Tasks and `journalctl -u machina-agent` on the host.
- Backing up before upgrades and upgrading the controller and daemon themselves: see the operator runbook (`docs/handbook/runbook.md`).

## Related pages

- [Maintenance](platform-maintenance.md)
- [Hosts](platform-hosts.md)
- [Tasks](platform-tasks.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
