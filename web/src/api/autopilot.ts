// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export interface Recommendation {
  vm_id: string
  name: string
  project: string
  vcpus: number
  memory_mib: number
  suggested_vcpus: number
  suggested_memory_mib: number
  cpu_p95: number
  mem_p95: number | null
  hours: number
  monthly_delta_usd: number
  reasons: string[]
  pending_action: string | null
}
export interface Rightsizing { recommendations: Recommendation[]; monthly_delta_usd: number; min_hours: number }
export interface HostLoad { id: string; name: string; memory_total_mib: number; memory_used_mib: number }
export interface ConsolidationMove { vm_id: string; vm: string; from: string; to: string; to_name: string }
export interface ConsolidationPlan { moves: ConsolidationMove[]; emptied: HostLoad[]; kept: [string, string][] }
export interface FiledAction { id: string; label: string; status: string }

const post = <T>(path: string, body: unknown) => platformFetch<T>(path, { method: 'POST', body: JSON.stringify(body) })
export const getRightsizing = () => platformFetch<Rightsizing>('/api/v1/rightsizing')
export const proposeResize = (vm_id: string) => post<FiledAction>('/api/v1/rightsizing/propose', { vm_id })
export const getConsolidation = () => platformFetch<ConsolidationPlan>('/api/v1/drs/consolidation')
export const proposeConsolidation = () => post<FiledAction>('/api/v1/drs/consolidation/propose', {})
