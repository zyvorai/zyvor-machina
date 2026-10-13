# Machina and the alternatives

A short guide to when Machina fits, written to help you decide, not to win an argument. Where we compare, we compare things we
can show; check each vendor's current documentation and pricing before you decide, because they change.

| You are choosing between | Machina fits when | The other fits when |
|---|---|---|
| **OpenStack** | You want a cloud one person can install and upgrade, on hardware you own, with an EC2-style API | You run thousands of tenants, need Neutron-grade SDN breadth, or depend on its ecosystem (detailed table in the [README](../../README.md)) |
| **A hypervisor with a management console** (VMware, Proxmox and similar) | You want open KVM/libvirt underneath, an EC2-compatible API, enforced security groups and scale-to-zero style features in one install | You need a vendor's certified hardware matrix, a particular partner ecosystem, or features we have not built |
| **Public cloud** | Data location, cost predictability or sovereignty require your own metal, and you like the EC2 workflow | You need global regions, managed services or elastic capacity you do not own |
| **Plain libvirt and scripts** | You have outgrown `virsh` scripts and want one API, UI, audit and fleet view | A handful of VMs on one host that never change |

## What Machina does not do today
Cross-host VPC forwarding proven on real traffic (peering, gateway routes and ACLs are enforced on the hosts, but not yet run across
hosts), per-interface security groups and a managed database or queue service. Multi-host failover is built and needs a host-loss drill on your hardware before you rely on it
([ledger](../claims.md)).

## How to find out for sure
Run the [30-day evaluation](evaluation-guide.md) on one host with your own workload. It is the only comparison that counts.
