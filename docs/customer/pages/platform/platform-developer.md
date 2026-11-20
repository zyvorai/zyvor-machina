# Developer

## Purpose

Developer Hub — Machina Platform page at `/platform/developer`.

## When to use it

- Operate **Developer** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/developer`
- Nav: **Platform → Developer Hub** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **SDK & Terraform / API Console**.
3. OpenAPI.
4. Try API Console.
5. **Empty:** —.
6. **Success:** Spec loads; call succeeds.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [API keys](platform-api-keys.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
