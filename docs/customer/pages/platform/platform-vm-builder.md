# AI VM Builder

## Purpose

VM Builder — Machina Platform page at `/platform/vm-builder`.

## When to use it

- Operate **AI VM Builder** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/vm-builder`
- Nav: **Platform → VM Builder** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Prompt → Suggest.
3. Create VM.
4. **Empty:** —.
5. **Success:** Queued create with sized spec.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Machina Zyra OS](../platform-security/platform-zyra.md)
- [Machine Finder](platform-vms.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
