# API keys

## Purpose

API Keys — Machina Platform page at `/platform/api-keys`.

## When to use it

- Operate **API keys** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/api-keys`
- Nav: **Platform → API Keys** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create key.
3. Rotate.
4. Delete.
5. **Empty:** Create bearer for CI.
6. **Success:** machina_* key shown once.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Developer](platform-developer.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
