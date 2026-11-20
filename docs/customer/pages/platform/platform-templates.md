# Marketplace (Templates)

## Purpose

Templates — Machina Platform page at `/platform/templates`.

## When to use it

- Operate **Marketplace (Templates)** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/templates`
- Nav: **Platform → Templates** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Fleet catalog**.
3. Sync git.
4. Restore defaults.
5. Refresh.
6. Launch from template.
7. **Empty:** Marketplace empty → sync/restore.
8. **Success:** Template usable in create.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Content Library](platform-content.md)
- [Mission Control](platform.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Page index](../../PAGE_INDEX.md)
