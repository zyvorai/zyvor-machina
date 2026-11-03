# Zyra Approvals

## Purpose

Approvals — Machina Platform / Security page at `/platform/zyra/approvals`.

## When to use it

- Operate **Zyra Approvals** when your job matches this page
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/zyra/approvals`
- Nav: **Platform / Security → Approvals** (or spotlight / Finder search)

## Notes

- Formerly documented under `/platform/zeus*` (except Zeus Security Center, which remains `/platform/zeus/security`).

## Operate from the console (UX)

1. Open the route against the Machina daemon (`https://<host>:5092`) and wait for live data.
2. Approve / Deny.
3. **Empty:** No pending approvals.
4. **Success:** Queue empty after approve.

If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Mission Control enrollment.

## Review before you approve

Each pending action has a **What happens if I approve?** line. Expand it to see a verdict (safe to approve, review first, or hold), the declared and assessed risk, whether the change can be undone and how, the machine, host and application groups in reach, and checks to do first (for example "make sure a recent backup exists"). It reads your data and changes nothing; the declared risk is never lowered. Needs the operator role.

## Fleet digest

Under the queue, **Fleet digest** summarises the last 24 hours, 7 days or 30 days: hosts not online, open incidents, pending approvals, what Zyra executed, failed and had undone, what a person rejected, and budget alerts. Use it to start the day or to hand over a shift.

## Related pages

- [Incident Commander](platform-zyra-incidents.md)
- [Tasks](../platform/platform-tasks.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
