# SSH — hypervisor

## Purpose

Host SSH — Machina Core page at `/host-ssh`.

## When to use it

- Operate **SSH — hypervisor** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/host-ssh`
- Nav: **Core → Host SSH** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Quick user cards**.
3. Pick user (root/admin).
4. Connect SSH console.
5. **Empty:** Hostname undetermined → back to Dashboard.
6. **Success:** Shell session connected.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Dashboard](home.md)
- [Host overview](../monitoring/node.md)
- [Getting Started](../../getting-started.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
