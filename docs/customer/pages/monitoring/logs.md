# System Logs

## Purpose

System Logs — Machina Monitoring page at `/logs`.

## When to use it

- Operate **System Logs** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/logs`
- Nav: **Monitoring → System Logs** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Unit/priority filters**.
3. Filter.
4. Follow journal.
5. **Empty:** —.
6. **Success:** Log lines stream.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Host overview](node.md)
- [Systemd Services](../core/services.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
