# Recommendations

## Purpose

Recommendations — Machina Platform page at `/platform/recommendations`.

## When to use it

- Operate **Recommendations** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/recommendations`
- Nav: **Platform → Recommendations** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Act on / dismiss recs.
3. **Empty:** Estate looks good.
4. **Success:** Rec applied or cleared.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [VM Rightsizing](../platform-security/platform-zyra-rightsizing.md)
- [Activity Monitor](platform-activity.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
