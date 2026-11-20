// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { CreateVmRequest } from '../api/vm'

const PREFIX = 'machina.createVm.defaults.v1'

export type CreateVmDefaultsPayload = Pick<
  CreateVmRequest,
  | 'vcpus'
  | 'memory_mb'
  | 'disk_gb'
  | 'network'
  | 'firmware'
  | 'graphics_type'
  | 'graphics_listen'
  | 'guest_profile'
  | 'os_variant'
  | 'virt_install_extra_args'
> & {
  /** UI install source tab — restored on load. */
  install_source?: 'iso' | 'url' | 'pxe' | 'download'
  /** UI storage mode */
  storage_mode?: 'new' | 'volume'
  disk_pool?: string
  cloud_init_user?: string
  cloud_init_ssh_pubkey?: string
  virtio_win_iso?: string
}

/** Stable key per hypervisor API identity (falls back to browser host). */
export function createVmDefaultsStorageKey(configuredUri?: string | null, browserHost?: string): string {
  const id = (configuredUri && configuredUri.trim()) || browserHost || 'default'
  try {
    return `${PREFIX}:${btoa(encodeURIComponent(id)).replace(/=+$/, '')}`
  } catch {
    return `${PREFIX}:default`
  }
}

export function loadCreateVmDefaults(key: string): Partial<CreateVmDefaultsPayload> | null {
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return null
    const v = JSON.parse(raw) as unknown
    return v && typeof v === 'object' ? (v as Partial<CreateVmDefaultsPayload>) : null
  } catch {
    return null
  }
}

export function saveCreateVmDefaults(key: string, payload: Partial<CreateVmDefaultsPayload>): void {
  try {
    localStorage.setItem(key, JSON.stringify(payload))
  } catch {
    /* quota / private mode */
  }
}
