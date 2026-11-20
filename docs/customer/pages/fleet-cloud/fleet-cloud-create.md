# Create Fleet Cloud instance

## Purpose

Create Instance — Machina Fleet Cloud page at `/fleet-cloud/create`.

## When to use it

- Operate **Create Fleet Cloud instance** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/create`
- Nav: **Fleet Cloud → Create Instance** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Wizard fields**.
3. Pick image/flavor/network/keypair/SGs.
4. Create instance.
5. **Empty:** No flavors → create flavor first.
6. **Success:** Instance listed running.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Images](fleet-cloud-images.md)
- [Flavors](fleet-cloud-flavors.md)
- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
