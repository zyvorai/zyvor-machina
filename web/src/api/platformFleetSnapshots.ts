// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export type FleetSnapshotSchedule = {
  id: string
  name: string
  cron_expr: string
  project: string
  tag_filter: string
  disk_only: boolean
  quiesce: boolean
  retain_count: number
  enabled: boolean
}

export const listFleetSnapshotSchedules = () =>
  platformFetch<FleetSnapshotSchedule[]>('/api/v1/fleet/snapshot-schedules')

export const createFleetSnapshotSchedule = (body: {
  name: string
  project?: string
  tag_filter?: string
  disk_only?: boolean
  quiesce?: boolean
}) =>
  platformFetch<FleetSnapshotSchedule>('/api/v1/fleet/snapshot-schedules', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const deleteFleetSnapshotSchedule = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/fleet/snapshot-schedules/${id}`, { method: 'DELETE' })
