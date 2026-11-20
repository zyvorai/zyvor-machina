// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export type CreatePlatformVmBody = {
  api_version: string
  kind: string
  metadata: { name: string; project?: string; labels?: Record<string, string> }
  spec: Record<string, unknown>
  host_id?: string
  tags?: string[]
  desired_state?: string
}

export const createPlatformVm = (body: unknown) =>
  platformFetch<{ task_id: string }>('/api/v1/vms', { method: 'POST', body: JSON.stringify(body) })

export type CreateFromIsoBody = {
  name: string
  iso_path: string
  memory?: string
  disk_gib?: number
  host_id?: string
  desired_state?: string
  cloud_init_user?: string
  cloud_init_password?: string
  cloud_init_ssh_pubkey?: string
}

export const createVmFromIso = (body: CreateFromIsoBody) =>
  platformFetch<{ task_id: string }>('/api/v1/vms/from-iso', {
    method: 'POST',
    body: JSON.stringify(body),
  })

export type CreateFromVirtInstallBody = {
  name: string
  memory?: string
  disk_gib?: number
  network?: string
  host_id?: string
  desired_state?: string
  os_variant?: string
  firmware?: string
  virt_install_location?: string
  virt_install_pxe?: boolean
  virt_install_pxe_network?: string
  virt_install_install_os?: string
  virt_install_extra_args?: string
  virt_install_define_only?: boolean
  existing_disk?: string
  root_disk_storage_pool?: string
  root_disk_storage_volume?: string
  virt_install_disk_backing_store?: string
  install_iso?: string
  virt_install_path_in_use_check_off?: boolean
  virt_install_unattended?: boolean
  virt_install_admin_password?: string
  virt_install_user_login?: string
  virt_install_user_password?: string
  cloud_init_user?: string
  cloud_init_password?: string
  cloud_init_ssh_pubkey?: string
}

export const createVmFromVirtInstall = (body: CreateFromVirtInstallBody) =>
  platformFetch<{ task_id: string }>('/api/v1/vms/from-virt-install', {
    method: 'POST',
    body: JSON.stringify(body),
  })
