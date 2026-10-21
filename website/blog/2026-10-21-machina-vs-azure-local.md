---
slug: machina-vs-azure-local
title: Machina vs Azure Local - where each fits
authors: [zyvor]
tags: [comparison, azure-local, hci]
description: An honest look at Azure Local and Machina, including what Machina does not do yet.
---

Azure Local (formerly Azure Stack HCI) has grown into a family of deployment models: hyperconverged, rack-aware, external SAN, disaggregated, multi-rack and disconnected operations. It is the right answer when Azure is your control plane. Machina is the right answer when you want open KVM, no cloud account and one install. This post lays out the difference.

{/* truncate */}

## Where Machina is already strong

- **No cloud dependency.** The control plane runs on your hosts. A local LLM provider is supported for AI operations.
- **Network security.** Native eBPF VM edge, security groups and microsegmentation, with signed evidence export.
- **Leaving VMware or Hyper-V.** [h2kvm](https://zyvor.dev/h2kvm) picks up VMs from vSphere, ESXi, Azure or any VMDK, VHDX, OVA or raw disk, repairs the guest offline with GuestKit, converts it and lands it on Machina.
- **Bare metal.** BMC power, PXE image deploy and join.
- **Price model.** A per-host subscription instead of a per-core host fee plus guest licensing.
- **Kubernetes and desktops.** Fleet k3s clusters (create, scale, upgrade, delete on enrolled hosts, opt-in) and simple desktop pools (clone, check out, re-clone). There is no connection broker and no Windows multi-session.
- **Air-gapped sites.** A guide lists every default internet touchpoint and the setting that points it at your mirror, with an egress check.

## Where Azure Local is ahead today

- Rack-aware clusters with a witness. Machina now has site and rack failure domains (placement spreads anti-affinity groups across racks, and a report shows whether HA could re-home a lost rack), but no witness or quorum arbiter.
- Built-in Storage Spaces Direct and SAN/disaggregated designs. Machina delegates storage to Atlas, NFS or local disks.
- Confidential VMs and attestation. Machina has vGPU partitions on hosts whose driver supports mediated devices, UEFI Secure Boot and an emulated TPM at VM create, but no SEV-SNP or TDX.
- Orchestrated disaster recovery. Machina copies backups to S3-compatible storage at another site and verifies them, with a manual restore runbook, but does not fail a site over.
- Windows 11 multi-session desktops and Microsoft workloads such as SQL Server on site.

Read the full [side-by-side table](/vs-azure-local). Every Machina statement traces to the [claims ledger](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md).
