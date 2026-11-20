// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, readJsonObject, apiPost, apiPostVoid, apiDelete } from './client'

const API = '/api/v1'

export interface BackupInfo {
  id: string
  timestamp: string
  vm_filter: string
  vm_count: number
  net_count: number
  with_disks: boolean
  nfs_target: string
  size: string
  status: string
  status_message: string
  progress: string
  has_checksums: boolean
  /** `fluxvm` for backups taken through `fluxvm-api`. */
  backend?: string
  size_bytes?: number
}

export interface BackupRequest {
  vm_name?: string
  with_disks?: boolean
  incremental?: boolean
  nfs_target?: string
  retain?: number
  /** `fluxvm`: back up a FluxVM VM (`vm_name` required). */
  backend?: string
  compress?: boolean
}

export interface RestoreRequest {
  backup_id: string
  backend?: string
  /** FluxVM: VM to restore into (must be stopped); defaults to the backup's own VM. */
  vm_name?: string
}

/** `?backend=fluxvm[&vm=…]` for FluxVM backup calls; '' for libvirt. */
export function backupScopeQs(backend?: string, vm?: string): string {
  const q = new URLSearchParams()
  if (backend) q.set('backend', backend)
  if (backend && vm) q.set('vm', vm)
  const s = q.toString()
  return s ? `?${s}` : ''
}

export interface BackupStatus {
  backup_id: string
  status: string
  message: string
  progress: string
  updated: string
}

export interface VerifyResult {
  backup_id: string
  verified: boolean
  files_checked: number
  files_ok: number
  files_failed: number
  failed_files: string[]
  errors: string
}

export interface ScheduleInfo {
  installed: boolean
  enabled: boolean
  active: boolean
  next_run: string
  last_run: string
}

export const fetchBackups = (backend?: string, vm?: string) =>
  readJsonArray<BackupInfo>(`${API}/backups${backupScopeQs(backend, vm)}`)

export const triggerBackup = (req: BackupRequest) =>
  apiPost<{ status: string; backup_id: string }>(`${API}/backups`, req)

export const restoreBackup = (req: RestoreRequest) =>
  apiPostVoid(`${API}/backups/restore`, req)

export const deleteBackup = (id: string, backend?: string) =>
  apiDelete(`${API}/backups/${encodeURIComponent(id)}${backupScopeQs(backend)}`)

export const getBackupStatus = (id: string) =>
  readJsonObject<BackupStatus>(`${API}/backups/${encodeURIComponent(id)}/status`)

export const verifyBackup = (id: string) =>
  apiPost<VerifyResult>(`${API}/backups/${encodeURIComponent(id)}/verify`)

export const downloadBackupUrl = (id: string) =>
  `${API}/backups/${encodeURIComponent(id)}/download`

export const getSchedule = () => readJsonObject<ScheduleInfo>(`${API}/backups/schedule`)

export const setSchedule = (enabled: boolean) =>
  apiPost<{ status: string; enabled: boolean }>(`${API}/backups/schedule`, { enabled })
