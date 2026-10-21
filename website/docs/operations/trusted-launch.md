---
sidebar_position: 9
title: Secure Boot and vTPM
description: Create VMs with UEFI Secure Boot enforced and an emulated TPM 2.0, the combination Windows 11 and hardened Linux images expect.
---

# Secure Boot and vTPM ("trusted launch")

Two options on VM create turn a plain UEFI VM into one that enforces signed boot and has a TPM:

| Option | Effect |
|---|---|
| `secure_boot: true` | UEFI with Secure Boot enforced and the Microsoft and distribution keys enrolled. Needs `firmware: "uefi"`. |
| `vtpm: true` | An emulated TPM 2.0 (`swtpm`, CRB interface). Works with BIOS or UEFI. |

Windows 11 needs both. Both default to off, so existing create calls behave as before.

```bash
curl -sk -X POST https://HOST:5092/api/v1/vms -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"name":"win11","vcpus":4,"memory_mb":8192,"disk_gb":80,"firmware":"uefi",
       "secure_boot":true,"vtpm":true,"iso":"/isos/Win11.iso","os_variant":"win11"}'
```

## How it works

Secure Boot uses libvirt's firmware auto-selection (`<os firmware='efi'>` with the `secure-boot` and `enrolled-keys` features) plus
System Management Mode (`<smm state='on'/>`), so libvirt chooses the signed OVMF build and a matching variable store from the host's
firmware descriptors. The `virt-install` create path passes the equivalent `--boot firmware=efi,...`, `--features smm=on` and
`--tpm` arguments. Machina checks the host first and refuses with a message that names the package to install:

- **Secure Boot:** a firmware descriptor under `/usr/share/qemu/firmware` or `/etc/qemu/firmware` must advertise both features
  (`edk2-ovmf` on Fedora/RHEL, `ovmf` on Debian/Ubuntu).
- **TPM:** `swtpm` (and `swtpm-tools`) must be installed.

Secure Boot VMs are created on the **q35** machine type (the signed firmware requires it); the `virt-install` path asks for it
explicitly.

An existing VM can still get a TPM later through the TPM attach call; it cannot be switched to Secure Boot in place.

## Limits

- x86_64 q35 only. Guests that do not support Secure Boot will not boot unsigned media: leave `secure_boot` off for those.
- The emulated TPM keeps its state on the host. It is not a hardware root of trust and does not provide remote attestation.
- Confidential VMs (AMD SEV-SNP, Intel TDX) are not offered.
- No web UI toggle yet; use the API or `machinactl`.
- Not yet run on a host: see claims C60.
