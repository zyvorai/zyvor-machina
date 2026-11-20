# Networks

## Purpose

The fleet network pane: libvirt networks across hosts, overlay segments with east-west policy, and IPAM pools that hand out addresses from a segment CIDR.

## When to use it

- Make an existing libvirt network (for example `virbr0`) known to the platform, or create a bridge-backed VM network
- Build Tier-0 (uplink) / Tier-1 (workload) overlay segments with east-west defaults
- Allocate addresses from IPAM pools instead of tracking them by hand

## How to get there

- Route: `/platform/networks`
- Nav: **Platform → Networks** (or spotlight / Finder search)

## What you can do

1. **Networks**: import libvirt networks from online hosts, or create a bridge-backed network. Search with the filter box; **Activate** / **Deactivate** / remove per row.
2. **Overlay segments**: **Create segment** with a CIDR, gateway, east-west default (allow or deny) and an optional Zyra firewall profile. Open a segment to see its allowed and blocked paths and micro-segmentation grade.
3. **IPAM pools**: segments with IPAM enabled show reservations and the next free offset.
4. **Emergency Unlock Segment** lifts a segment's east-west deny when it is blocking legitimate traffic during an incident.

## If something is wrong

- **No networks yet:** import from a host that is online, or create one.
- East-west policy here is simulated and planned at the platform level; host-level packet enforcement is done by [Native eBPF](../platform-security/platform-zyra-security-native-bpf.md).

## Related pages

- [Networks](../infrastructure/networks.md)
- [Network canvas](platform-network-canvas.md)
- [Security Center](../platform-security/platform-zeus-security.md)
- [Native eBPF](../platform-security/platform-zyra-security-native-bpf.md)
- [Getting Started](../../getting-started.md)
- [Dashboard](../core/home.md)
- [Mission Control](platform.md)
- [Page index](../../PAGE_INDEX.md)
