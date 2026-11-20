# Web sessions

## Purpose

Web Sessions — Machina Monitoring page at `/admin/sessions`.

## When to use it

- Operate **Web sessions** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/admin/sessions`
- Nav: **Monitoring → Web Sessions** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Revoke (root only).
3. **Empty:** No active sessions.
4. **Success:** Session list matches logins.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Audit Log](audit.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
