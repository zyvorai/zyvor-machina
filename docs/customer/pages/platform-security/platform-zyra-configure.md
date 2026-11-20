# Configure Zyra

## Purpose

Configure Zyra — Machina Platform / Security page at `/platform/zyra/configure`.

## When to use it

- Operate **Configure Zyra** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/zyra/configure`
- Nav: **Platform / Security → Configure Zyra** (or spotlight / Finder search)

## Notes

- Formerly documented under `/platform/zeus*` (except Zeus Security Center, which remains `/platform/zeus/security`).

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Save air-gap.
3. Enable memory.
4. Save prompt.
5. **Empty:** —.
6. **Success:** Settings saved.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machina Zyra OS](platform-zyra.md)
- [AI Providers](../platform/platform-ai-providers.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
