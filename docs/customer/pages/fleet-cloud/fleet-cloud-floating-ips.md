# Floating IPs

## Purpose

Floating IPs — Machina Fleet Cloud page at `/fleet-cloud/floating-ips`.

## When to use it

- Operate **Floating IPs** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/floating-ips`
- Nav: **Fleet Cloud → Floating IPs** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Instance forwards**.
3. Add forward.
4. Refresh.
5. **Empty:** No native FIP pool — use port forward.
6. **Success:** Forward works to guest.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Fleet Cloud Instances](fleet-cloud-instances.md)
- [Networking](fleet-cloud-networking.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
