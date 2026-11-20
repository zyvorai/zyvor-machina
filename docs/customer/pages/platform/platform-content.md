# Content Library

## Purpose

Images & ISOs — Machina Platform page at `/platform/content`.

## When to use it

- Operate **Content Library** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/content`
- Nav: **Platform → Images & ISOs** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **All / Pending / Approved**.
3. Upload.
4. Download URL.
5. Create from ISO.
6. Approve.
7. **Empty:** No images → upload for approval.
8. **Success:** Image Approved.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create VM from ISO](platform-create-iso.md)
- [Marketplace (Templates)](platform-templates.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
