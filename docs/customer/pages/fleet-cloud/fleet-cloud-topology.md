# Network topology

## Purpose

Network Topology — Machina Fleet Cloud page at `/fleet-cloud/topology`.

## When to use it

- Operate **Network topology** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/topology`
- Nav: **Fleet Cloud → Network Topology** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Inspect graph.
3. **Empty:** Empty until nets/instances.
4. **Success:** Topology shows nets+VMs.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Networking](fleet-cloud-networking.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
