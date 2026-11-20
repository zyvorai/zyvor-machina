# Virtual Machines

## Purpose

Virtual machine inventory for this libvirt host.

## When to use it

- Operate **Virtual Machines** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/vms`
- Nav: **Core → Virtual Machines** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Search; tag filter; Table/Grid; pin**.
3. Create VM.
4. Import VM.
5. Start/Stop/Pause/Resume.
6. Console/SSH.
7. Bulk delete.
8. Export JSON/CSV.
9. **Empty:** No guests → Create VM / Import.
10. **Success:** VM appears running; console opens.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create new guest VM](create.md)
- [Import guest VM](import.md)
- [VM detail: Guest policy](vm-guest-policy.md)
- [Machine Finder](../platform/platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
