---
slug: machina-vs-azure-local
title: Machina vs Azure Local - where each fits
authors: [zyvor]
tags: [comparison, azure-local, hci]
description: An honest look at Azure Local and Machina, including what Machina does not do yet.
---

Azure Local (formerly Azure Stack HCI) has grown into a family of deployment models: hyperconverged, rack-aware, external SAN, disaggregated, multi-rack and disconnected operations. It is the right answer when Azure is your control plane. Machina is the right answer when you want open KVM, no cloud account and one install. This post lays out the difference.

<!-- truncate -->

## Where Machina is already strong

- **No cloud dependency.** The control plane runs on your hosts. A local LLM provider is supported for AI operations.
- **Network security.** Native eBPF VM edge, security groups and microsegmentation, with signed evidence export.
- **Leaving VMware or Hyper-V.** [h2kvm](https://zyvor.dev/h2kvm) picks up VMs from vSphere, ESXi, Azure or any VMDK, VHDX, OVA or raw disk, repairs the guest offline with GuestKit, converts it and lands it on Machina.
- **Bare metal.** BMC power, PXE image deploy and join.
- **Price model.** A per-host subscription instead of a per-core host fee plus guest licensing.

## Where Azure Local is ahead today

- Rack-aware clusters and a witness (planned for Machina as failure domains).
- Built-in Storage Spaces Direct and SAN/disaggregated designs. Machina delegates storage to Atlas, NFS or local disks.
- GPU partitioning, Trusted Launch and confidential VMs.
- Windows 11 multi-session desktops and Microsoft workloads such as SQL Server on site.

Read the full [side-by-side table](/vs-azure-local). Every Machina statement traces to the [claims ledger](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md).
