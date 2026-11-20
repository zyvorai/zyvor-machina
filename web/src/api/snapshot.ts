// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, apiPostVoid, apiDelete } from './client'
import { appendVmConnection } from './vm'

const API = '/api/v1'

export interface SnapshotInfo {
  name: string
  vm_name: string
  creation_time: number
  state: string
  description: string
  parent: string
  is_current: boolean
}

export const listAllSnapshots = () => readJsonArray<SnapshotInfo>(`${API}/snapshots`)
export const listSnapshots = (vm: string, connection?: string | null) =>
  readJsonArray<SnapshotInfo>(
    appendVmConnection(`${API}/vms/${encodeURIComponent(vm)}/snapshots`, connection),
  )

export interface SnapshotDiskSpec {
  name: string
  snapshot?: 'no' | 'external' | 'internal' | 'manual' | string
  file?: string
  driver?: string
}

interface CreateSnapshotRequest {
  name: string
  description?: string
  disk_only?: boolean
  storage_mode?: 'auto' | 'external' | 'internal' | string
  memory_snapshot?: 'internal' | 'external' | string
  memory_file?: string
  external_disk_dir?: string
  external_memory_dir?: string
  disks?: SnapshotDiskSpec[]
  atomic?: boolean
  reuse_external?: boolean
}

export const createSnapshot = (vm: string, body: CreateSnapshotRequest, connection?: string | null) =>
  apiPostVoid(appendVmConnection(`${API}/vms/${encodeURIComponent(vm)}/snapshots`, connection), body)
export const deleteSnapshot = (vm: string, snap: string, connection?: string | null) =>
  apiDelete(
    appendVmConnection(`${API}/vms/${encodeURIComponent(vm)}/snapshots/${encodeURIComponent(snap)}`, connection),
  )
export const revertSnapshot = (vm: string, snap: string, connection?: string | null) =>
  apiPostVoid(
    appendVmConnection(`${API}/vms/${encodeURIComponent(vm)}/snapshots/${encodeURIComponent(snap)}/revert`, connection),
  )
