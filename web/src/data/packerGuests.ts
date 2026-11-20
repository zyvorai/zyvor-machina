// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Golden-image guests for `contrib/packer/build-linux-image.sh` and
 * matching `virt-install --install os=…` short-ids (libosinfo).
 */

type PackerGuestFamily = 'rpm' | 'debian' | 'ubuntu' | 'windows'

interface MachinaPackerScriptGuest {
  /** First argument to `build-linux-image.sh` */
  id: string
  label: string
  family: PackerGuestFamily
  /** Suggested `virt-install --os-variant` when cloning this golden */
  osVariantHint: string
  /** Valid `virt-install --install os=…` value for “Automatic OS install” */
  virtInstallDownloadOs: string
  defaultLoginUser: 'root' | 'packer'
  /** Short operator hint (not a spec dump) */
  notes?: string
}

/** Guests implemented by `contrib/packer/build-linux-image.sh` today. */
export const MACHINA_PACKER_SCRIPT_GUESTS: readonly MachinaPackerScriptGuest[] = [
  {
    id: 'fedora43',
    label: 'Fedora 43',
    family: 'rpm',
    osVariantHint: 'fedora43',
    virtInstallDownloadOs: 'fedora43',
    defaultLoginUser: 'packer',
    notes: 'Kickstart over HTTP; SSH as packer during build.',
  },
  {
    id: 'ubuntu2204',
    label: 'Ubuntu 22.04 LTS',
    family: 'ubuntu',
    osVariantHint: 'ubuntu22.04',
    virtInstallDownloadOs: 'ubuntu22.04',
    defaultLoginUser: 'packer',
  },
  {
    id: 'ubuntu2404',
    label: 'Ubuntu 24.04 LTS',
    family: 'ubuntu',
    osVariantHint: 'ubuntu24.04',
    virtInstallDownloadOs: 'ubuntu24.04',
    defaultLoginUser: 'packer',
  },
  {
    id: 'ubuntu2504',
    label: 'Ubuntu 25.04',
    family: 'ubuntu',
    osVariantHint: 'ubuntu25.04',
    virtInstallDownloadOs: 'ubuntu25.04',
    defaultLoginUser: 'packer',
    notes: 'Interim release.',
  },
  {
    id: 'ubuntu2510',
    label: 'Ubuntu 25.10',
    family: 'ubuntu',
    osVariantHint: 'ubuntu25.10',
    virtInstallDownloadOs: 'ubuntu25.10',
    defaultLoginUser: 'packer',
    notes: 'Interim release.',
  },
  {
    id: 'ubuntu2604',
    label: 'Ubuntu 26.04 LTS',
    family: 'ubuntu',
    osVariantHint: 'ubuntu26.04',
    virtInstallDownloadOs: 'ubuntu26.04',
    defaultLoginUser: 'packer',
    notes: 'When available on mirrors; confirm osinfo on your host if install fails.',
  },
  {
    id: 'debian12',
    label: 'Debian 12',
    family: 'debian',
    osVariantHint: 'debian12',
    virtInstallDownloadOs: 'debian12',
    defaultLoginUser: 'packer',
  },
  {
    id: 'debian13',
    label: 'Debian 13',
    family: 'debian',
    osVariantHint: 'debian13',
    virtInstallDownloadOs: 'debian13',
    defaultLoginUser: 'packer',
  },
  {
    id: 'almalinux9',
    label: 'AlmaLinux 9',
    family: 'rpm',
    osVariantHint: 'almalinux9',
    virtInstallDownloadOs: 'almalinux9',
    defaultLoginUser: 'root',
  },
  {
    id: 'rocky9',
    label: 'Rocky Linux 9',
    family: 'rpm',
    osVariantHint: 'rocky9',
    virtInstallDownloadOs: 'rocky9',
    defaultLoginUser: 'root',
  },
  {
    id: 'centos9stream',
    label: 'CentOS Stream 9',
    family: 'rpm',
    osVariantHint: 'centos-stream9',
    virtInstallDownloadOs: 'centos-stream9',
    defaultLoginUser: 'root',
  },
  {
    id: 'oraclelinux9',
    label: 'Oracle Linux 9',
    family: 'rpm',
    osVariantHint: 'oraclelinux9',
    virtInstallDownloadOs: 'oraclelinux9',
    defaultLoginUser: 'root',
  },
  {
    id: 'win11',
    label: 'Windows 11',
    family: 'windows',
    osVariantHint: 'win11',
    virtInstallDownloadOs: 'win11',
    defaultLoginUser: 'root',
    notes: 'dockur/windows via Podman; default login Docker/admin — rotate before production.',
  },
  {
    id: 'win10',
    label: 'Windows 10',
    family: 'windows',
    osVariantHint: 'win10',
    virtInstallDownloadOs: 'win10',
    defaultLoginUser: 'root',
    notes: 'dockur/windows via Podman; default login Docker/admin — rotate before production.',
  },
  {
    id: 'windows-server-2022',
    label: 'Windows Server 2022',
    family: 'windows',
    osVariantHint: 'win2k22',
    virtInstallDownloadOs: 'win2k22',
    defaultLoginUser: 'root',
    notes: 'dockur/windows VERSION=2022; default login Docker/admin — rotate before production.',
  },
  {
    id: 'windows-server-2025',
    label: 'Windows Server 2025',
    family: 'windows',
    osVariantHint: 'win2k25',
    virtInstallDownloadOs: 'win2k25',
    defaultLoginUser: 'root',
    notes: 'dockur/windows VERSION=2025; default login Docker/admin — rotate before production.',
  },
] as const

export const PACKER_SCRIPT_SYSTEM = '/usr/local/share/machina/packer/build-linux-image.sh'
export const PACKER_SCRIPT_REPO = 'contrib/packer/build-linux-image.sh'
export const PACKER_WINDOWS_DOCKUR_SYSTEM =
  '/usr/local/share/machina/packer/build-windows-dockur.sh'
export const PACKER_WINDOWS_DOCKUR_REPO = 'contrib/packer/build-windows-dockur.sh'
