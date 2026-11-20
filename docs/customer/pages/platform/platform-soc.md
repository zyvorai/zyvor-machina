# Security Operations Center

## Purpose

Security Operations — Machina Platform page at `/platform/soc`.

## When to use it

- Operate **Security Operations Center** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/soc`
- Nav: **Platform → Security Operations** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Filters / tabs: **Overview / Alerts / Detections / Attack surface / Integrations**.
3. Run ingest.
4. New playbook.
5. Enable forwarders.
6. **Empty:** —.
7. **Success:** Alerts/detections populate.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [Security Center](../platform-security/platform-zeus-security.md)
- [Mission Control](platform.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Page index](../../PAGE_INDEX.md)
