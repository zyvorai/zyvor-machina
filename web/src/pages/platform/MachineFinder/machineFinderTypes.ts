// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type MachineFinderLens =
  | 'grid'
  | 'gallery'
  | 'table'
  | 'topology'
  | 'timeline'
  | 'heatmap'
  | 'migration'

export type MachineFinderOverlay =
  | 'default'
  | 'health'
  | 'backup'
  | 'network'
  | 'security'
  | 'gpu'
  | 'cost'
  | 'migration'

export const CLIENT_ONLY_FOLDERS = new Set(['guest-gaps', 'guest_agent_missing'])

export const SOURCE_LABELS: Record<string, string> = {
  libvirt: 'Libvirt',
  kubevirt: 'KubeVirt',
  vmware: 'VMware',
  vsphere: 'VMware',
  proxmox: 'Proxmox',
  discovered: 'Discovered',
}
