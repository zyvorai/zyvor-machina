// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export interface PreemptSettings { enabled: boolean; reserve_pct: number }

export interface PreemptHost {
  id: string
  name: string
  total_mib: number
  used_mib: number
  free_mib: number
  reserve_mib: number
  preemptible_running_mib: number
  fresh: boolean
}

export interface PreemptibleVm {
  id: string
  name: string
  host_id: string | null
  host: string | null
  project: string | null
  memory_mib: number
  priority: number
  desired_state: string
  observed_state: string
  preempted_at: string | null
}

export interface PreemptEvent { vm: string; kind: string; reason: string; at: string }

export interface PreemptOverview {
  settings: PreemptSettings
  resume_margin_pct: number
  hosts: PreemptHost[]
  vms: PreemptibleVm[]
  events: PreemptEvent[]
}

const put = <T>(path: string, body: unknown) => platformFetch<T>(path, { method: 'PUT', body: JSON.stringify(body) })
export const getPreemption = () => platformFetch<PreemptOverview>('/api/v1/preemption')
export const updatePreemptSettings = (s: PreemptSettings) => put<PreemptSettings>('/api/v1/preemption/settings', s)
export const setVmPreemptible = (id: string, preemptible: boolean, priority: number) =>
  put<{ preemptible: boolean; priority: number }>(`/api/v1/vms/${id}/preemptible`, { preemptible, priority })
