# Container Pods

## Purpose

Podman pods via Vessel — create and manage shared-namespace container groups (not Kubernetes pods).

## When to use it

- Operate **Container Pods** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/containers/pods`
- Nav: **Infrastructure → Container Pods** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Create pod.
3. Start/Stop.
4. Refresh.
5. **Empty:** No pods.
6. **Success:** Pod running.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Containers](containers.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
