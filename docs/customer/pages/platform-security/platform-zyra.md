# Machina Zyra OS

## Purpose

Zyra AI assistant for Machina operations.

## When to use it

- Operate **Machina Zyra OS** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/zyra`
- Nav: **Platform / Security → Zyra AI** (or spotlight / Finder search)

## Notes

- Formerly documented under `/platform/zeus*` (except Zeus Security Center, which remains `/platform/zeus/security`).

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Fleet / Graph Brain / Memory / Security / Knowledge (?tab=)**.
3. Open Firewall.
4. Register BMC.
5. Run runbooks.
6. Open apps.
7. **Empty:** No bare-metal → register BMC.
8. **Success:** Tab data loads; sensors enrolled.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Configure Zyra](platform-zyra-configure.md)
- [Security Center](platform-zeus-security.md)
- [Incident Commander](platform-zyra-incidents.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
