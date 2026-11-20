# Images

## Purpose

Images — Machina Fleet Cloud page at `/fleet-cloud/images`.

## When to use it

- Operate **Images** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/images`
- Nav: **Fleet Cloud → Images** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Register image.
3. Delete.
4. Refresh.
5. **Empty:** Empty catalog → register.
6. **Success:** Image ACTIVE for boot.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create Fleet Cloud instance](fleet-cloud-create.md)
- [Import guest VM](../core/import.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
