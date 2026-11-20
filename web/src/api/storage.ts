// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, apiPost, apiPostVoid, apiDelete } from './client'

const API = '/api/v1'

export interface StoragePoolInfo {
  name: string
  uuid: string
  state: string
  capacity_gb: number
  allocation_gb: number
  available_gb: number
  autostart: boolean
}

export interface StorageVolumeInfo {
  name: string
  pool: string
  capacity_gb: number
  allocation_gb: number
  path: string
  vol_type: string
}

interface CreateVolumeRequest {
  name: string
  capacity_gb: number
  format: string
}

export const listPools = () => readJsonArray<StoragePoolInfo>(`${API}/storage/pools`)
export const startPool = (name: string) => apiPostVoid(`${API}/storage/pools/${encodeURIComponent(name)}/start`)
export const stopPool = (name: string) => apiPostVoid(`${API}/storage/pools/${encodeURIComponent(name)}/stop`)
export const refreshPool = (name: string) => apiPostVoid(`${API}/storage/pools/${encodeURIComponent(name)}/refresh`)
export const setPoolAutostart = (name: string, enabled: boolean) => apiPostVoid(`${API}/storage/pools/${encodeURIComponent(name)}/autostart/${enabled}`)
export const listVolumes = (pool: string) =>
  readJsonArray<StorageVolumeInfo>(`${API}/storage/pools/${encodeURIComponent(pool)}/volumes`)
export const createVolume = (pool: string, req: CreateVolumeRequest) => apiPost<unknown>(`${API}/storage/pools/${encodeURIComponent(pool)}/volumes`, req)
export const deleteVolume = (pool: string, vol: string) => apiDelete(`${API}/storage/pools/${encodeURIComponent(pool)}/volumes/${encodeURIComponent(vol)}`)
