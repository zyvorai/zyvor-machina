# Security Center

## Purpose

Fleet security overview: the infrastructure security graph, critical findings, natural-language event search, and the eBPF sensor matrix showing `machina-bpfd` on every host as reported through its agent.

## When to use it

- Daily security review of the fleet
- Search events in plain language ("show every process that opened port 8080 last week")
- Check that every host's eBPF sensor is reporting

## How to get there

- Route: `/platform/zeus/security`
- Nav: **Security Center** (or spotlight / Finder search)

## What you can do

1. **Critical** / **Requires attention** summarise findings that need action.
2. **Infrastructure security graph** links hosts, VMs, networks and findings.
3. **Zeus security search** takes a natural-language query and returns matching events.
4. **eBPF sensor matrix** / **Sensor registry** list each host's `machina-bpfd` (version, mode, health). A host shown as **machina-bpfd unreachable** has no running bpfd or no agent link.
5. **Open Native eBPF** goes to the per-host console. The sub-nav also has Kubernetes, Cloud SGs, Connectivity, Policy Studio, Threat Hunting and Enforcement.

## If something is wrong

- **machina-bpfd unreachable:** on that host, `systemctl status machina-bpfd machina-agent`.
- **Search returns nothing:** check that AI providers are configured for Zyra and that hosts are sending events.

## Related pages

- [Native eBPF](platform-zyra-security-native-bpf.md)
- [Security Operations Center](../platform/platform-soc.md)
- [Machina Zyra OS](platform-zyra.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](../platform/platform.md)
- [Page index](../../PAGE_INDEX.md)
