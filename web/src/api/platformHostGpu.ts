// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export type HostGpuDevice = {
  pci_address: string
  vendor: string
  device_name: string
  iommu_group: number
  mig_profile: string
}

export const getHostGpus = (hostId: string) =>
  platformFetch<{ devices: HostGpuDevice[]; nvidia_smi_summary: string }>(`/api/v1/hosts/${hostId}/gpus`)
