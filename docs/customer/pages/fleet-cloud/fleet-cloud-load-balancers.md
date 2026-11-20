# Load Balancers

## Purpose

Load Balancers — Machina Fleet Cloud page at `/fleet-cloud/load-balancers`.

## When to use it

- Operate **Load Balancers** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/load-balancers`
- Nav: **Fleet Cloud → Load Balancers** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create.
3. Add members.
4. **Empty:** No load balancers.
5. **Success:** LB listening with members.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
