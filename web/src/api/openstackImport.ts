// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiDelete, apiGet, apiPost } from './client'
import { PLATFORM_CONTROLLER_PROXY } from './platform'

const base = `${PLATFORM_CONTROLLER_PROXY}/api/v1/openstack`

export type OsConnection = {
  id: string
  name: string
  auth_url: string
  username: string
  project_name: string
  region: string
  insecure_tls: boolean
  status: string
  last_error: string
}

export type OsInventoryItem = {
  kind: string
  remote_id: string
  name: string
  status: string
  size_bytes?: number | null
  disk_format?: string
}

export async function listOsConnections() {
  return apiGet<OsConnection[]>(`${base}/connections`)
}

export async function createOsConnection(body: {
  name: string
  auth_url: string
  username?: string
  password?: string
  project_name?: string
  insecure_tls?: boolean
}) {
  return apiPost<OsConnection>(`${base}/connections`, body)
}

export async function deleteOsConnection(id: string) {
  return apiDelete(`${base}/connections/${id}`)
}

export async function syncOsInventory(id: string) {
  return apiPost<{ count: number; items: OsInventoryItem[] }>(`${base}/connections/${id}/inventory`, {})
}

export async function listOsInventory(id: string) {
  return apiGet<{ items: OsInventoryItem[] }>(`${base}/connections/${id}/inventory`)
}

export async function submitOsImport(
  connectionId: string,
  body: { kind: string; remote_id: string; dest_name?: string; host_id?: string },
) {
  return apiPost<{ job_id: string; status: string }>(`${base}/connections/${connectionId}/imports`, body)
}

export async function getOsImportJob(id: string) {
  return apiGet<{
    id: string
    status: string
    progress: number
    error: string
    template_id?: string | null
  }>(`${base}/imports/${id}`)
}
