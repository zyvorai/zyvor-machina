# Activity Monitor

## Purpose

Activity Monitor — Machina Platform page at `/platform/activity`.

## When to use it

- Operate **Activity Monitor** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/activity`
- Nav: **Platform → Activity Monitor** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **VMs / Hosts**.
3. Refresh.
4. Switch scope.
5. **Empty:** No running VMs / No hosts.
6. **Success:** Live CPU/mem/PSI.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machine Finder](platform-vms.md)
- [Hosts](platform-hosts.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
