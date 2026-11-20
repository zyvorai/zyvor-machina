# Preemptible

## Purpose

Preemptible instances are VMs that can wait. When a host runs short of memory, the lowest-priority preemptible VMs are saved to disk instead of stopped, and they come back by themselves, lowest priority first, when there is room again.

## When to use it

- Open this page when the job matches the purpose above
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/preemptible`
- Nav: **Fleet Cloud → Preemptible** (or spotlight / Finder search)

## What you can do

1. See which VMs are marked preemptible and their priority, and which are currently saved.
2. Turn preemption on or off for the platform and mark or unmark a VM.
3. Watch the events that explain why a VM was saved or resumed.
4. See [Fleet Cloud features](../../../fleet-cloud-features.md) for how pressure is detected and what is guaranteed.


If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Launchpad to be enabled.

## Related pages

- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
