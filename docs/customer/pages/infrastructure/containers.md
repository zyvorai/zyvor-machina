# Containers

## Purpose

Local Podman/Docker containers via Vessel — list, create, lifecycle, and live stats on this host.

## When to use it

- Operate **Containers** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/containers`
- Nav: **Infrastructure → Containers** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create container.
3. Start/Stop.
4. Refresh.
5. **Empty:** No containers (Vessel/Podman/Docker).
6. **Success:** Container running.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Container Pods](containers-pods.md)
- [Dashboard](../core/home.md)
- [Getting Started](../../getting-started.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
