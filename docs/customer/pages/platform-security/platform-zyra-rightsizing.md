# VM Rightsizing

## Purpose

Rightsizing — Machina Platform / Security page at `/platform/zyra/rightsizing`.

## When to use it

- Operate **VM Rightsizing** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/zyra/rightsizing`
- Nav: **Platform / Security → Rightsizing** (or spotlight / Finder search)

## Notes

- Formerly documented under `/platform/zeus*` (except Zeus Security Center, which remains `/platform/zeus/security`).

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Queue single resize.
3. **Empty:** —.
4. **Success:** Resize task queued.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Recommendations](../platform/platform-recommendations.md)
- [Machine Finder](../platform/platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
