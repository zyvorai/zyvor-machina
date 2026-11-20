# Kubernetes Workloads

## Purpose

K8s Workloads — Machina Kubernetes page at `/k8s/workloads`.

## When to use it

- Operate **Kubernetes Workloads** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/k8s/workloads`
- Nav: **Kubernetes → K8s Workloads** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Deployments/pods/services**.
3. Refresh.
4. Scale/rollout.
5. Apply.
6. Delete.
7. **Empty:** No workloads in this scope.
8. **Success:** Deployment Ready.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Kubernetes](k8s.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
