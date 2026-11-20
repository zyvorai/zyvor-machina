# Machine Finder

## Purpose

Machine Finder: opens the fleet VM list in its topology lens (/platform/vms?lens=topology) to find any VM by host, network or name.

## When to use it

- Open this page when the job matches the purpose above
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/hosts/finder`
- Nav: **Platform → Machine Finder** (or spotlight / Finder search)

## What you can do

1. Open `/platform/hosts/finder` against the Machina daemon (`https://<host>:5092`).
2. Use filters and host/VM selectors when the page provides them.
3. Drill into a VM, host, or Fleet Cloud resource for consoles and detail panels.
4. For mutating actions (create/delete VM, apply firewall, Fleet Cloud change): confirm the target host and role (Admin/Operator).

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Launchpad to be enabled.

## Related pages

- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
