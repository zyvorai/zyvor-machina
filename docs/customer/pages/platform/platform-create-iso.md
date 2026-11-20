# Create VM from ISO

## Purpose

Create ISO — Machina Platform page at `/platform/create-iso`.

## When to use it

- Operate **Create VM from ISO** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/create-iso`
- Nav: **Platform → Create ISO** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Network: Default DHCP**.
3. Create.
4. Import .pub.
5. Open Content Library.
6. **Empty:** No approved ISOs → Content Library.
7. **Success:** VM task queued; appears in Finder.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Content Library](platform-content.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
