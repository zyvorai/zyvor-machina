# Dashboard

## Purpose

Host dashboard — VM inventory pulse, Vessel container shortcuts when Podman/Docker is connected, and quick links.

## When to use it

- Operate **Dashboard** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/`
- Nav: **Core → Dashboard** (or spotlight / Finder search)

## Notes

- If control-plane proxy URL is set, `/` may redirect to `/platform` (Mission Control).

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Guest strip; K8s status card**.
3. Refresh.
4. Create VM.
5. Create container.
6. Reboot/Shutdown host.
7. Open Platform.
8. Run system check.
9. **Empty:** Stats show 0 guests — use Create VM.
10. **Success:** Guests count + running badge update; metrics populate.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Virtual Machines](vms.md)
- [Create new guest VM](create.md)
- [Mission Control](../platform/platform.md)
- [System Check](../monitoring/system-check.md)
- [Containers](../infrastructure/containers.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Page index](../../PAGE_INDEX.md)
