# Heat Orchestration

## Purpose

Heat Orchestration — Machina Fleet Cloud page at `/fleet-cloud/heat`.

## When to use it

- Operate **Heat Orchestration** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/heat`
- Nav: **Fleet Cloud → Heat Orchestration** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create stack.
3. Delete.
4. **Empty:** No stacks.
5. **Success:** Stack CREATE_COMPLETE.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Fleet Cloud](fleet-cloud.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
