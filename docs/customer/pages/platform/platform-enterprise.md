# Enterprise Features

## Purpose

Enterprise Features — Machina Platform page at `/platform/enterprise`.

## When to use it

- Operate **Enterprise Features** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/enterprise`
- Nav: **Platform → Enterprise Features** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Secrets / FIPS etc.**.
3. Register provider.
4. Sync.
5. Save policy.
6. **Empty:** —.
7. **Success:** Policy saved.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Settings](platform-settings.md)
- [Secrets](../infrastructure/secrets.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
