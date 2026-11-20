# Topology

## Purpose

Topology — Machina Platform page at `/platform/topology`.

## When to use it

- Operate **Topology** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/topology`
- Nav: **Platform → Topology** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Enroll host.
3. Import networks.
4. **Empty:** Enroll hosts and import networks.
5. **Success:** Twin graph populated.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Datacenter](platform-datacenter.md)
- [Networks](platform-networks.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
