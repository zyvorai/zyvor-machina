# Volumes

## Purpose

Volumes — Machina Fleet Cloud page at `/fleet-cloud/volumes`.

## When to use it

- Operate **Volumes** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/volumes`
- Nav: **Fleet Cloud → Volumes** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Project**.
3. Create volume.
4. Attach/Detach.
5. Delete.
6. Refresh.
7. **Empty:** No volumes in this project.
8. **Success:** Attached to instance.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Volume Snapshots](fleet-cloud-volume-snapshots.md)
- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
