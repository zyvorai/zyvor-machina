// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonObject, apiPut } from './client'
import { parseResponseError } from './parseResponseError'

const API = '/api/v1'

export interface OsUserCapability {
  canCreateOsUsers: boolean
  /** Same gate as create; omitted on older daemons (treat like canCreateOsUsers). */
  canDeleteOsUsers?: boolean
  reason: string | null
  /** Host has `getent group libvirt` (typical for qemu:///system). */
  libvirtGroupAvailable?: boolean
  libvirtGroupName?: string
  /** `"traditional"` or `"systemd-homed"` — how new accounts are provisioned. */
  userAccountBackend?: string
  systemdHomedActive?: boolean
  homectlAvailable?: boolean
  /** `wheel` or `sudo` when present in NSS (used for sudo-capable supplementary group). */
  sudoSupplementaryGroup?: string | null
}

interface CreateOsUserResult {
  libvirt_group_attached: boolean
  account_backend?: string
}

export async function getOsUserCapability(): Promise<OsUserCapability> {
  return readJsonObject<OsUserCapability>(`${API}/system/os-users/capability`)
}

export async function createOsUser(
  username: string,
  password: string,
  addToLibvirtGroup = true,
): Promise<CreateOsUserResult> {
  const res = await fetch(`${API}/system/os-users`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password, add_to_libvirt_group: addToLibvirtGroup }),
  })
  if (!res.ok) {
    throw await parseResponseError(res)
  }
  const body = (await res.json().catch(() => ({}))) as {
    libvirt_group_attached?: boolean
    account_backend?: string
  }
  return {
    libvirt_group_attached: Boolean(body.libvirt_group_attached),
    account_backend: body.account_backend,
  }
}

export async function deleteOsUser(username: string): Promise<void> {
  const enc = encodeURIComponent(username)
  const res = await fetch(`${API}/system/os-users/${enc}`, {
    method: 'DELETE',
    credentials: 'same-origin',
  })
  if (!res.ok) {
    throw await parseResponseError(res)
  }
}

/** Server-wide Create VM form defaults (`/var/lib/machina/create-vm-defaults.json`). */
export async function getServerCreateVmDefaults(): Promise<Record<string, unknown>> {
  return readJsonObject<Record<string, unknown>>(`${API}/system/create-vm-defaults`)
}

export async function putServerCreateVmDefaults(body: Record<string, unknown>): Promise<{ status?: string }> {
  return apiPut(`${API}/system/create-vm-defaults`, body)
}

/** Mirrors `GET /api/v1/system/platform-info` — runtime capability summary used by the shell. */
export interface PlatformInfo {
  version: string
  host?: {
    os_pretty_name?: string
  }
  tls: { enabled: boolean }
  auth: {
    pam_service: string
    ldap_enabled?: boolean
    oidc_enabled: boolean
    run_as_user_enabled?: boolean
    run_as_user_mode?: string
  }
  fleet?: { enabled: boolean; peer_count: number }
  rdp?: { builtin_ws_proxy: boolean }
  libvirt?: { dual_connection: boolean; extra_uris?: string[] }
  kubevirt: {
    exec_enabled: boolean
    default_namespace: string
    default_storage_class: string
    virtio_container_disk_image: string
    machine_type: string
  }
  fluxvm?: {
    enabled: boolean
    base_url: string
    default_backend: string
  }
  guestkit?: {
    enabled: boolean
    base_url: string
    insecure_tls: boolean
  }
  native_bpf?: {
    socket: string
  }
  zeus_firewall?: {
    enabled: boolean
    phase: number
  }
  control_plane?: {
    proxy_url: string
    direct_url: string
  }
}

export const getPlatformInfo = () => readJsonObject<PlatformInfo>(`${API}/system/platform-info`)
