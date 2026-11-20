# Users & Groups

## Purpose

Users & Groups — Machina Platform page at `/platform/users`.

## When to use it

- Operate **Users & Groups** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/users`
- Nav: **Platform → Users & Groups** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Workspaces**.
3. Add user.
4. Delete.
5. Open Machine Finder.
6. **Empty:** No workspaces yet.
7. **Success:** User listed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Stage Manager](platform-projects.md)
- [API keys](platform-api-keys.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
