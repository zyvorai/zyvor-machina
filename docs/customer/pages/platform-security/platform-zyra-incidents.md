# Incident Commander

## Purpose

Incident Commander — Machina Platform / Security page at `/platform/zyra/incidents`.

## When to use it

- Operate **Incident Commander** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/zyra/incidents`
- Nav: **Platform / Security → Incident Commander** (or spotlight / Finder search)

## Notes

- Formerly documented under `/platform/zeus*` (except Zeus Security Center, which remains `/platform/zeus/security`).

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Open approvals.
3. Runbook.
4. **Empty:** —.
5. **Success:** Incident advanced/closed.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Draft a postmortem

On an open or closed incident, **Draft postmortem** downloads a Markdown file: severity, status, detection window, summary, impact, root cause, a timeline and the Zyra actions proposed in that window. Anything the platform does not know is stated as missing, not filled in. Read it and edit it before you share it.

## Related pages

- [Zyra Approvals](platform-zyra-approvals.md)
- [Security Operations Center](../platform/platform-soc.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
