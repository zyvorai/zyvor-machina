# Machine Finder

## Purpose

Fleet VM finder across enrolled hosts.

## When to use it

- Operate **Machine Finder** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/vms`
- Nav: **Platform → Virtual Machines** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Lenses: Grid/Gallery/Table/Topology/Timeline/Heatmap/Migration; Smart Folders; tags**.
3. New VM.
4. Open Cinema.
5. SSH.
6. Power.
7. Delete.
8. Prune missing.
9. **Empty:** No machines → create VM / change folder.
10. **Success:** VM in folder; power state updates.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Mission Control](platform.md)
- [Hosts](platform-hosts.md)
- [Create VM from ISO](platform-create-iso.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Page index](../../PAGE_INDEX.md)
