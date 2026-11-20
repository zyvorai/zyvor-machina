# Cloud-Init Studio

## Purpose

Cloud-Init Studio — Machina Platform page at `/platform/cloud-init`.

## When to use it

- Operate **Cloud-Init Studio** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/cloud-init`
- Nav: **Platform → Cloud-Init Studio** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Edit.
3. Validate #cloud-config.
4. **Empty:** —.
5. **Success:** Validation OK.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create VM from ISO](platform-create-iso.md)
- [Create new guest VM](../core/create.md)
- [Create Fleet Cloud instance](../fleet-cloud/fleet-cloud-create.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
