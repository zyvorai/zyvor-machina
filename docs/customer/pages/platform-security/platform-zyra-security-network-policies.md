# VM Network Policies

## Purpose

VM network policies decide which VM may talk to which, in both directions, using the CiliumNetworkPolicy schema. They compile into the eBPF edge Machina attaches to each VM's network interface; Cilium does not need to be installed.

## When to use it

- Open this page when the job matches the purpose above
- Use Mission Control (`/platform`) for fleet-wide work; use Core routes for this host only
- Confirm PAM/OIDC login and roles if actions are missing

## How to get there

- Route: `/platform/zyra/security/network-policies`
- Nav: **Platform / Security → VM Network Policies** (or spotlight / Finder search)

## What you can do

1. Write or paste policy YAML and use the dry-run preview to see which VMs and rules it selects.
2. Test a connection (source, destination, port) against the policy set before enforcing anything.
3. Browse endpoints and label selectors, and watch live connections in the Flows terminal.
4. See [VM network policy](../../../ebpf/vm-network-policy.md) for the supported fields and how enforcement fails open.


If the page stays empty, check daemon health (`/api/v1/health`), libvirt connectivity, and whether the feature requires Fleet Cloud or Launchpad to be enabled.

## Related pages

- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
