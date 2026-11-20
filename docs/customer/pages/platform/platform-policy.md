# Policy rules

## Purpose

Policy — Machina Platform page at `/platform/policy`.

## When to use it

- Operate **Policy rules** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/policy`
- Nav: **Platform → Policy** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Save quota.
3. **Empty:** —.
4. **Success:** Quota saved.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Users & Groups](platform-users.md)
- [Stage Manager](platform-projects.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
