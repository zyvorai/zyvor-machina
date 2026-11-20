# Server Groups

## Purpose

Server Groups — Machina Fleet Cloud page at `/fleet-cloud/server-groups`.

## When to use it

- Operate **Server Groups** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/server-groups`
- Nav: **Fleet Cloud → Server Groups** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Add to group.
3. Delete.
4. Refresh.
5. **Empty:** No anti-affinity groups yet.
6. **Success:** Group has members.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
