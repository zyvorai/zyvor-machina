# Systemd Services

## Purpose

Services — Machina Core page at `/services`.

## When to use it

- Operate **Systemd Services** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/services`
- Nav: **Core → Services** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Start/Stop/Enable units.
3. View journal.
4. **Empty:** —.
5. **Success:** Unit Active.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Host overview](../monitoring/node.md)
- [System Logs](../monitoring/logs.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
