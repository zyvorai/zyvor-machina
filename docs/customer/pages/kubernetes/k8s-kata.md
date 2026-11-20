# Kata + Cloud Hypervisor

## Purpose

Kata + Cloud Hypervisor — Machina Kubernetes page at `/k8s/kata`.

## When to use it

- Operate **Kata + Cloud Hypervisor** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/k8s/kata`
- Nav: **Kubernetes → Kata + Cloud Hypervisor** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Install sequence.
3. Apply RuntimeClass.
4. Refresh contexts.
5. **Empty:** Need cluster context.
6. **Success:** RuntimeClass present; pod with kata-clh.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Kubernetes](k8s.md)
- [Kubernetes Workloads](k8s-workloads.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
