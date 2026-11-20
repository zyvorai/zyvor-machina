// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Atlas storage control-plane API — talks to the controller's /api/v1/atlas/*
// proxy over the platform (daemon→controller) channel.

import { platformFetch } from './platform'

export interface AtlasStatus {
  enabled: boolean
  base_url: string
  reachable: boolean
  authenticated: boolean
  version?: string | null
  tenant_id: string
  default_policy: string
  backup_bucket_id?: string | null
  summary: string
}

export interface AtlasBackend {
  id: string
  name: string
  backend_type: string
  mode: string
  status: string
  capabilities: Record<string, boolean>
}

export interface AtlasVolume {
  id: string
  cluster_id?: string | null
  pool_id?: string | null
  name: string
  kind?: string | null
  backend_native_id?: string | null
  size_bytes?: number | null
  used_bytes?: number | null
  state?: string | null
  health?: string | null
}

export interface AtlasJob {
  id?: string | null
  job_id?: string | null
  job_type?: string | null
  state: string
  progress_percent?: number | null
  error?: string | null
  result?: unknown
  resource?: unknown
}

export interface AtlasSnapshot {
  id: string
  tenant_id?: string
  volume_id: string
  name: string
  state: string
  protected?: boolean
  created_at?: string
}

export interface AtlasBackup {
  id: string
  tenant_id?: string
  volume_id: string
  snapshot_id?: string
  bucket_id?: string
  object_key?: string
  format?: string
  checksum?: string
  state: string
  created_at?: string
}

const A = '/api/v1/atlas'

export const getAtlasStatus = () => platformFetch<AtlasStatus>(`${A}/status`)
export const listAtlasBackends = () => platformFetch<AtlasBackend[]>(`${A}/backends`)
export const listAtlasVolumes = (query?: Record<string, string>) => {
  const qs = query ? `?${new URLSearchParams(query).toString()}` : ''
  return platformFetch<AtlasVolume[]>(`${A}/volumes${qs}`)
}
export const listAtlasSnapshots = () => platformFetch<AtlasSnapshot[]>(`${A}/snapshots`)
export const listAtlasBackups = (volumeId?: string) =>
  platformFetch<AtlasBackup[]>(`${A}/backups${volumeId ? `?volume_id=${encodeURIComponent(volumeId)}` : ''}`)
export const listAtlasJobs = () => platformFetch<AtlasJob[]>(`${A}/jobs`)

export const createAtlasVolume = (body: {
  name: string
  size_bytes: number
  policy?: string
  storage_class?: string
  namespace?: string
}) => platformFetch<AtlasJob>(`${A}/volumes`, { method: 'POST', body: JSON.stringify(body) })

export const deleteAtlasVolume = (id: string) =>
  platformFetch<unknown>(`${A}/volumes/${id}`, { method: 'DELETE' })

export const snapshotAtlasVolume = (id: string, name?: string) =>
  platformFetch<AtlasJob>(`${A}/volumes/${id}/snapshots`, {
    method: 'POST',
    body: JSON.stringify(name ? { name } : {}),
  })

export const restoreAtlasSnapshot = (id: string, name?: string) =>
  platformFetch<AtlasJob>(`${A}/snapshots/${id}/restore`, {
    method: 'POST',
    body: JSON.stringify(name ? { name } : {}),
  })

export const deleteAtlasSnapshot = (id: string, force = false) =>
  platformFetch<unknown>(`${A}/snapshots/${id}${force ? '?force=true' : ''}`, { method: 'DELETE' })

export const backupAtlasVolume = (body: {
  volume_id: string
  bucket_id?: string
  mode?: string
  keep?: number
}) => platformFetch<AtlasJob>(`${A}/backups`, { method: 'POST', body: JSON.stringify(body) })

export const deleteAtlasBackup = (id: string) =>
  platformFetch<unknown>(`${A}/backups/${id}`, { method: 'DELETE' })
