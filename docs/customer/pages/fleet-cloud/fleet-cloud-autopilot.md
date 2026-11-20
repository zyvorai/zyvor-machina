# Autopilot

## Purpose

Autopilot is capacity management for Fleet Cloud: instance groups scale ahead of the daily or weekly rush, drain from their load balancer, and sleep instead of stopping; VMs are right-sized from their own history and hosts can be consolidated.

## When to use it

- Open this page when the job matches the purpose above
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/autopilot`
- Nav: **Fleet Cloud → Autopilot** (or spotlight / Finder search)

## What you can do

1. Review the forecast-based scaling the autopilot proposes for each instance group.
2. Review right-sizing and host-consolidation recommendations, each with its evidence.
3. Approve a recommendation, or undo one that was applied.
4. See [Fleet Cloud features](../../../fleet-cloud-features.md) for what runs automatically and what always waits for approval.


If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Launchpad to be enabled.

## Related pages

- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
