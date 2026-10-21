# Machina and the alternatives

A short guide to when Machina fits, written to help you decide, not to win an argument. Where we compare, we compare things we
can show; check each vendor's current documentation and pricing before you decide, because they change.

| You are choosing between | Machina fits when | The other fits when |
|---|---|---|
| **OpenStack** | You want a cloud one person can install and upgrade, on hardware you own, with an EC2-style API | You run thousands of tenants, need Neutron-grade SDN breadth, or depend on its ecosystem (detailed table in the [README](../../README.md)) |
| **A hypervisor with a management console** (VMware, Proxmox and similar) | You want open KVM/libvirt underneath, an EC2-compatible API, enforced security groups and scale-to-zero style features in one install | You need a vendor's certified hardware matrix, a particular partner ecosystem, or features we have not built |
| **Azure Local** (Azure Stack HCI) | You want open KVM, no cloud account, a per-host price and eBPF network enforcement ([comparison](https://zyvorai.github.io/zyvor-machina/vs-azure-local)) | Azure is your control plane, or you need rack-aware clusters, SAN or disaggregated storage, Windows 11 multi-session desktops or SQL Server on site |
| **Nutanix** | You already run Ceph or NFS and want the control plane, with eBPF segmentation and an EC2-style API | You want one vendor for compute, storage and built-in replication |
| **Public cloud** | Data location, cost predictability or sovereignty require your own metal, and you like the EC2 workflow | You need global regions, managed services or elastic capacity you do not own |
| **Plain libvirt and scripts** | You have outgrown `virsh` scripts and want one API, UI, audit and fleet view | A handful of VMs on one host that never change |

## Moving in from another platform
Transiva reads vSphere, Hyper-V, Nutanix, Proxmox, AWS, Azure, GCP, OCI, OpenStack and Alibaba; h2kvm converts the disks and
GuestKit repairs the guest offline before first boot ([guide](https://zyvorai.github.io/zyvor-machina/docs/getting-started/migrate-vms-in)).
These are separate Zyvor products, not part of the Machina install.

## What Machina does not do today
Rack-aware placement and a witness, iSCSI/Fibre Channel targets, vGPU partitioning, desktop pools (VDI), orchestrated site
failover, Arc-style management of foreign estates, cross-host VPCs, an internet-gateway datapath beyond host-local Elastic IPs and NAT, per-interface security groups and a managed
database or queue service. Multi-host failover is built and needs a host-loss drill on your hardware before you rely on it
([ledger](../claims.md)).

## How to find out for sure
Run the [30-day evaluation](evaluation-guide.md) on one host with your own workload. It is the only comparison that counts.
