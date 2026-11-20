# Fleet

## Purpose

Fleet — Machina Core page at `/fleet`.

## When to use it

- Operate **Fleet** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet`
- Nav: **Core → Fleet** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Peer list; placement vCPU/mem**.
3. Refresh.
4. Create on best peer.
5. Start/Shutdown/Stop peer VMs.
6. Placement suggest.
7. **Empty:** No fleet peers → add peers in Settings.
8. **Success:** Peers online; VMs aggregated.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Virtual Machines](vms.md)
- [Hosts](../platform/platform-hosts.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
