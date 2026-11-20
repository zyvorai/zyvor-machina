# Advanced VM install

## Purpose

Advanced Create — Machina Platform page at `/platform/create-advanced`.

## When to use it

- Operate **Advanced VM install** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/create-advanced`
- Nav: **Platform → Advanced Create** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Default DHCP**.
3. virt-install path/URL.
4. Create from ISO.
5. **Empty:** —.
6. **Success:** Install task running.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create VM from ISO](platform-create-iso.md)
- [Create new guest VM](../core/create.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
