# Kubernetes

## Purpose

Kubernetes — Machina Kubernetes page at `/k8s`.

## When to use it

- Operate **Kubernetes** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/k8s`
- Nav: **Kubernetes → Kubernetes** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Cluster/node status**.
3. Refresh contexts.
4. Open workloads.
5. Open Security Center.
6. **Empty:** No cluster nodes.
7. **Success:** Nodes Ready.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Kubernetes Workloads](k8s-workloads.md)
- [Kata + Cloud Hypervisor](k8s-kata.md)
- [Security Center](../platform-security/platform-zeus-security.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
