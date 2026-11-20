# Sprites

## Purpose

Instant, disposable sandbox VMs — boot on libvirt/QEMU, Cloud Hypervisor, or Firecracker, TTL-reaped automatically, no persistent state.

## When to use it

- Operate **Sprites** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/sprites`
- Nav: **Core → Sprites** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. New Sprite.
3. Refresh.
4. Delete.
5. **Empty:** No sprites running → New Sprite.
6. **Success:** Sprite running with TTL.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Virtual Machines](vms.md)
- [Create new guest VM](create.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
