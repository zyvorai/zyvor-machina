# AI Providers

## Purpose

AI Providers — Machina Platform page at `/platform/ai-providers`.

## When to use it

- Operate **AI Providers** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/ai-providers`
- Nav: **Platform → AI Providers** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Add provider.
3. Save.
4. Delete.
5. **Empty:** —.
6. **Success:** Provider usable by Zyra.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machina Zyra OS](../platform-security/platform-zyra.md)
- [Settings](platform-settings.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
