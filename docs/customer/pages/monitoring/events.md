# Live Metrics

## Purpose

Live Metrics — Machina Monitoring page at `/events`.

## When to use it

- Operate **Live Metrics** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/events`
- Nav: **Monitoring → Live Metrics** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Per-VM metrics**.
3. Refresh.
4. **Empty:** No running VMs with metrics.
5. **Success:** Charts update for running guests.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Host overview](node.md)
- [Virtual Machines](../core/vms.md)
- [Activity Monitor](../platform/platform-activity.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
