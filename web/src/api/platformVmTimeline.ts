// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Per-VM backup + snapshot timeline (Time Machine).

import { platformFetch } from './platform'

export type VmTimelineEntry = {
  kind: 'backup' | 'snapshot' | string
  id: string
  label: string
  status: string
  created_at: string
}

export const listVmTimeline = (vmId: string) =>
  platformFetch<VmTimelineEntry[]>(`/api/v1/vms/${vmId}/timeline`)
