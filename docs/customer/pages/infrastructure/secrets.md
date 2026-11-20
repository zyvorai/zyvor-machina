# Secrets

## Purpose

Secrets — Machina Infrastructure page at `/secrets`.

## When to use it

- Operate **Secrets** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/secrets`
- Nav: **Infrastructure → Secrets** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create/delete secret.
3. **Empty:** No secrets found.
4. **Success:** Secret listed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Enterprise Features](../platform/platform-enterprise.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
