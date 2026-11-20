# Daemon Jobs

## Purpose

Daemon Jobs — Machina Monitoring page at `/jobs`.

## When to use it

- Operate **Daemon Jobs** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/jobs`
- Nav: **Monitoring → Daemon Jobs** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Job list**.
3. Refresh.
4. Open job.
5. Create VM link.
6. **Empty:** No jobs yet.
7. **Success:** Job succeeded.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create new guest VM](../core/create.md)
- [Disk Images](../infrastructure/disk-images.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
