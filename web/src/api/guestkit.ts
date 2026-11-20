// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'
import { readJsonArray, readJsonObject } from './client'

const DAEMON_API = '/api/v1'

export interface GuestkitStatus {
  enabled: boolean
  base_url: string
  insecure_tls: boolean
  reachable: boolean
  last_error?: string
  library?: string
}

export interface GuestkitDoctorReport {
  image_path: string
  target: string
  boot_score: number
  confidence: number
  summary: string
  blockers: string[]
  warnings: string[]
  checks_passed: number
  checks_total: number
  root_cause?: string
}

export interface GuestkitMigratePlanReport {
  image_path: string
  target: string
  migration_score: number
  boot_score: number
  estimated_downtime_minutes: number
  driver_injections: string[]
  required_changes: string[]
  licensing_warnings: string[]
  summary: string
}

export const getGuestkitDaemonStatus = () =>
  readJsonObject<GuestkitStatus>(`${DAEMON_API}/guestkit/status`)

export const getGuestkitStatus = () => platformFetch<GuestkitStatus & { library_version: string; worker_url: string; worker_reachable: boolean; summary: string }>(
  '/api/v1/guestkit/status',
)

export const guestkitDoctor = (image_path: string, target = 'kvm', explain = false) =>
  platformFetch<GuestkitDoctorReport>('/api/v1/guestkit/doctor', {
    method: 'POST',
    body: JSON.stringify({ image_path, target, explain }),
  })

export const guestkitMigratePlan = (image_path: string, target = 'kvm') =>
  platformFetch<GuestkitMigratePlanReport>('/api/v1/guestkit/migrate-plan', {
    method: 'POST',
    body: JSON.stringify({ image_path, target }),
  })

export const guestkitVmDoctor = (vmId: string, target = 'kvm', explain = false) => {
  const q = new URLSearchParams({ target })
  if (explain) q.set('explain', 'true')
  return platformFetch<GuestkitDoctorReport>(`/api/v1/guestkit/vms/${encodeURIComponent(vmId)}/doctor?${q}`)
}

export const guestkitVmMigratePlan = (vmId: string, target = 'kvm') => {
  const q = new URLSearchParams({ target })
  return platformFetch<GuestkitMigratePlanReport>(`/api/v1/guestkit/vms/${encodeURIComponent(vmId)}/migrate-plan?${q}`)
}

export const submitGuestkitInspectJob = (image_path: string, name = 'machina-inspect') =>
  platformFetch<{ job_id: string; status: string; summary: string }>('/api/v1/guestkit/jobs', {
    method: 'POST',
    body: JSON.stringify({ image_path, name }),
  })

export const getGuestkitJob = (id: string) =>
  platformFetch<{ job_id: string; status: string; progress?: number; summary?: string; result?: Record<string, unknown> }>(
    `/api/v1/guestkit/jobs/${id}`,
  )

export interface GuestkitJobRow {
  job_id: string
  status: string
  summary?: string
  created_at?: string
}

interface GuestkitCapabilities {
  features: string[]
  summary: string
}

export const listGuestkitJobsDaemon = () =>
  readJsonArray<GuestkitJobRow>(`${DAEMON_API}/guestkit/jobs`)

export const getGuestkitCapabilitiesDaemon = () =>
  readJsonObject<GuestkitCapabilities>(`${DAEMON_API}/guestkit/capabilities`)
