# Security Groups

## Purpose

Security Groups — Machina Fleet Cloud page at `/fleet-cloud/security-groups`.

## When to use it

- Operate **Security Groups** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/security-groups`
- Nav: **Fleet Cloud → Security Groups** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create group.
3. Add rules.
4. Delete.
5. **Empty:** No security groups.
6. **Success:** Rules applied; attachable.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Create Fleet Cloud instance](fleet-cloud-create.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
