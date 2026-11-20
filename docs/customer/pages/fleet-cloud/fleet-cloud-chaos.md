# Game days

## Purpose

Game days inject faults into your own VMs under a lease: network latency and loss, partitions, slow disks and crashes. Health probes abort the run and lift every fault when the service suffers, and each step gets a report.

## When to use it

- Open this page when the job matches the purpose above
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/fleet-cloud/chaos`
- Nav: **Fleet Cloud → Game days** (or spotlight / Finder search)

## What you can do

1. Define an experiment: the target VMs, the faults, their duration and the health probes that must stay green.
2. Start a run; faults are leased and lifted automatically when the lease ends or a probe fails.
3. Read the per-step report of what was injected and how the service responded.
4. Only run game days on VMs you are allowed to disturb; the faults are real.


If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Launchpad to be enabled.

## Related pages

- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
