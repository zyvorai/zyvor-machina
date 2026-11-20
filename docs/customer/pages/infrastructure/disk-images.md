# Disk Images

## Purpose

Disk Images — Machina Infrastructure page at `/disk-images`.

## When to use it

- Operate **Disk Images** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/disk-images`
- Nav: **Infrastructure → Disk Images** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create VM.
3. Import VM.
4. Open Jobs.
5. **Empty:** No disk images found.
6. **Success:** Image path usable in create/import.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create new guest VM](../core/create.md)
- [Import guest VM](../core/import.md)
- [Daemon Jobs](../monitoring/jobs.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
