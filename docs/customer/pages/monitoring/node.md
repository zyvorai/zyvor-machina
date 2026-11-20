# Host overview

## Purpose

Host Overview — Machina Monitoring page at `/node`.

## When to use it

- Operate **Host overview** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/node`
- Nav: **Monitoring → Host Overview** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Resource gauges; history charts**.
3. Refresh.
4. Install/start libvirt.
5. Kill process.
6. Package upgrade links.
7. **Empty:** Could not load host info.
8. **Success:** CPU/mem/disk gauges live.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Live Metrics](events.md)
- [Systemd Services](../core/services.md)
- [System Logs](logs.md)
- [Host Networking](../infrastructure/host-networking.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
