# Maintenance

## Purpose

Maintenance — Machina Platform page at `/platform/maintenance`.

## When to use it

- Operate **Maintenance** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/maintenance`
- Nav: **Platform → Maintenance** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Schedule window.
3. Apply upgrades.
4. Enroll host.
5. **Empty:** No hosts / No windows.
6. **Success:** Window scheduled.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Upgrade Guide](platform-upgrade.md)
- [Host Enrollment](platform-enroll.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
