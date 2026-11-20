# GPU Command Center

## Purpose

GPU Command Center — Machina Platform page at `/platform/gpu`.

## When to use it

- Operate **GPU Command Center** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/gpu`
- Nav: **Platform → GPU Command Center** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Browse hosts.
3. Create VM here.
4. Open Zyra.
5. **Empty:** No GPU inventory → tag hosts.
6. **Success:** GPUs discovered.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Hosts](platform-hosts.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
