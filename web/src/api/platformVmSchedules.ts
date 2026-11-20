// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export type VmScheduleAction = 'start' | 'shutdown' | 'stop' | 'snapshot'

export type VmSchedule = {
  id: string
  vm_id: string
  action: VmScheduleAction
  interval_minutes: number
  retention: number | null
  label: string
  enabled: boolean
  next_run_at: string
  last_run_at: string | null
  created_at: string
}

export const listVmSchedules = (vmId: string) =>
  platformFetch<VmSchedule[]>(`/api/v1/vms/${vmId}/schedules`)

export const createVmSchedule = (
  vmId: string,
  body: {
    action: VmScheduleAction
    interval_minutes: number
    retention?: number | null
    label?: string
  }
) =>
  platformFetch<VmSchedule>(`/api/v1/vms/${vmId}/schedules`, {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const deleteVmSchedule = (vmId: string, scheduleId: string) =>
  platformFetch<{ ok: boolean }>(`/api/v1/vms/${vmId}/schedules/${scheduleId}`, {
    method: 'DELETE',
  })
