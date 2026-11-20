# Stage Manager

## Purpose

Stage Manager — Machina Platform page at `/platform/projects`.

## When to use it

- Operate **Stage Manager** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/projects`
- Nav: **Platform → Stage Manager** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Spaces**.
3. Open Machine Finder.
4. Assign project labels.
5. **Empty:** No workspace spaces yet.
6. **Success:** Space shows VMs.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machine Finder](platform-vms.md)
- [Users & Groups](platform-users.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
