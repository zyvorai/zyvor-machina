# Fleet Cloud

## Purpose

Overview — Machina Fleet Cloud page at `/fleet-cloud`.

## When to use it

- Operate **Fleet Cloud** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud`
- Nav: **Fleet Cloud → Overview** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Sub-nav**.
3. Create instance.
4. Open Instances/Images/Volumes quick links.
5. **Empty:** —.
6. **Success:** Cards navigate; counts sensible.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Create Fleet Cloud instance](fleet-cloud-create.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
