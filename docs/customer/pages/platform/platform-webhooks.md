# Webhooks

## Purpose

Webhooks — Machina Platform page at `/platform/webhooks`.

## When to use it

- Operate **Webhooks** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/webhooks`
- Nav: **Platform → Webhooks** (or spotlight / Finder search)

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Add endpoint.
3. Delete.
4. View deliveries.
5. **Empty:** No webhook endpoints.
6. **Success:** Delivery succeeds.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Related pages

- [API keys](platform-api-keys.md)
- [Platform events](platform-events.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
