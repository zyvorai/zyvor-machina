// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Day-2 controller operations: backup schedules, alert rules, scheduled jobs,
// API-key rotation, and host cordon/drain.

import { platformFetch } from './platform'

// ---------------------------------------------------------------------------
// Backup schedules
// ---------------------------------------------------------------------------

export type BackupSchedule = {
  id: string
  name: string
  project: string
  tag_filter: string
  backup_type: string
  target_id: string
  interval_hours: number
  retain_count: number
  enabled: boolean
  last_run_at?: string | null
  created_at: string
}

export const listBackupSchedules = () =>
  platformFetch<BackupSchedule[]>('/api/v1/backup-schedules')

export const createBackupSchedule = (body: {
  name: string
  project?: string
  tag_filter?: string
  backup_type?: string
  target_id?: string
  interval_hours?: number
  retain_count?: number
  enabled?: boolean
}) =>
  platformFetch<BackupSchedule>('/api/v1/backup-schedules', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const deleteBackupSchedule = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/backup-schedules/${encodeURIComponent(id)}`, { method: 'DELETE' })

// ---------------------------------------------------------------------------
// Alert rules
// ---------------------------------------------------------------------------

export type AlertMetric =
  | 'cpu_percent'
  | 'mem_percent'
  // fleet-wide metrics (the default rules use these; scope is ignored)
  | 'host_offline'
  | 'storage_pool_percent'
  | 'backup_failed_24h'
  | 'failed_task_burst'
export type AlertComparator = 'gt' | 'lt'
export type AlertSeverity = 'info' | 'warning' | 'critical'

export type AlertRule = {
  id: string
  name: string
  metric: AlertMetric | string
  comparator: AlertComparator | string
  threshold: number
  severity: AlertSeverity | string
  scope_project: string
  scope_tag: string
  cooldown_minutes: number
  enabled: boolean
  last_fired_at?: string | null
  created_at: string
}

export const listAlertRules = () =>
  platformFetch<AlertRule[]>('/api/v1/alert-rules')

export const createAlertRule = (body: {
  name: string
  metric: AlertMetric
  comparator: AlertComparator
  threshold: number
  severity: AlertSeverity
  scope_project?: string
  scope_tag?: string
  cooldown_minutes?: number
  enabled?: boolean
}) =>
  platformFetch<AlertRule>('/api/v1/alert-rules', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export type NotificationChannelRow = {
  id: string
  name: string
  kind: 'slack' | 'email' | 'webhook' | string
  target: string
  events: string
  enabled: boolean
}

export const listNotificationChannelRows = () =>
  platformFetch<NotificationChannelRow[]>('/api/v1/notification-channels')

export const createNotificationChannel = (body: { name: string; kind: string; target: string }) =>
  platformFetch<NotificationChannelRow>('/api/v1/notification-channels', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const testNotificationChannel = (id: string) =>
  platformFetch<{ ok?: boolean; message?: string }>(`/api/v1/notification-channels/${encodeURIComponent(id)}/test`, {
    method: 'POST',
  })

/** A channel's target can embed a secret (a Slack webhook URL): show where it goes, never the whole address. */
export function channelTargetLabel(kind: string, target: string): string {
  if (kind === 'email') return target
  try {
    return new URL(target).host
  } catch {
    return '(set)'
  }
}

export const setAlertRuleEnabled = (id: string, enabled: boolean) =>
  platformFetch<AlertRule>(`/api/v1/alert-rules/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify({ enabled }),
  })

export const deleteAlertRule = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/alert-rules/${encodeURIComponent(id)}`, { method: 'DELETE' })

// ---------------------------------------------------------------------------
// Scheduled jobs
// ---------------------------------------------------------------------------

export type ScheduledJob = {
  id: string
  name: string
  operation: string
  payload: string
  target_host_id: string
  interval_minutes: number
  enabled: boolean
  last_run_at?: string | null
  created_at: string
}

/** Whitelisted operations the controller accepts for scheduled jobs. */
export const SCHEDULED_JOB_OPERATIONS = ['host.inventory'] as const

export const listScheduledJobs = () =>
  platformFetch<ScheduledJob[]>('/api/v1/scheduled-jobs')

export const createScheduledJob = (body: {
  name: string
  operation: string
  payload?: string
  target_host_id?: string
  interval_minutes?: number
  enabled?: boolean
}) =>
  platformFetch<ScheduledJob>('/api/v1/scheduled-jobs', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export const setScheduledJobEnabled = (id: string, enabled: boolean) =>
  platformFetch<ScheduledJob>(`/api/v1/scheduled-jobs/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify({ enabled }),
  })

export const deleteScheduledJob = (id: string) =>
  platformFetch<{ deleted: boolean }>(`/api/v1/scheduled-jobs/${encodeURIComponent(id)}`, { method: 'DELETE' })

// ---------------------------------------------------------------------------
// API-key rotation
// ---------------------------------------------------------------------------

export const rotateApiKey = (id: string) =>
  platformFetch<{ id: string; name: string; role: string; token: string }>(
    `/api/v1/api-keys/${encodeURIComponent(id)}/rotate`,
    { method: 'POST', body: '{}' },
  )

// ---------------------------------------------------------------------------
// Host cordon / drain
// ---------------------------------------------------------------------------

export const cordonHost = (id: string, cordon: boolean) =>
  platformFetch<{ host_id: string; schedulable: boolean }>(`/api/v1/hosts/${encodeURIComponent(id)}/cordon`, {
    method: 'POST',
    body: JSON.stringify({ cordon }),
  })
