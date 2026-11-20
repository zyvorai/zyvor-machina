# Fleet Cloud Instances

## Purpose

Instances — Machina Fleet Cloud page at `/fleet-cloud/instances`.

## When to use it

- Operate **Fleet Cloud Instances** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/instances`
- Nav: **Fleet Cloud → Instances** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Project scope**.
3. Create instance.
4. Start/Stop/Reboot.
5. Refresh.
6. Console.
7. **Empty:** No instances → Create.
8. **Success:** Instance ACTIVE.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Create Fleet Cloud instance](fleet-cloud-create.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
