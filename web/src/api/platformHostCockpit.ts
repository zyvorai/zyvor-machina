// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'
import { formatHttpErrorBody } from '../utils/apiError'

const API = '/api/v1'

type CockpitStorageLine = { name: string; detail: string; state: string }

export type HostCockpitStorage = {
  probed: boolean
  mdraid: CockpitStorageLine[]
  luks: CockpitStorageLine[]
  lvm: CockpitStorageLine[]
  stratis: CockpitStorageLine[]
  vdo: CockpitStorageLine[]
  multipath: CockpitStorageLine[]
  iscsi: CockpitStorageLine[]
  summary: string
}

type NmConnection = {
  name: string
  uuid: string
  kind: string
  device: string
  state: string
}

type FirewalldZone = {
  name: string
  target: string
  services: string[]
  ports: string[]
}

export type HostCockpitNetwork = {
  probed: boolean
  connections: NmConnection[]
  bonds: NmConnection[]
  teams: NmConnection[]
  bridges: NmConnection[]
  vlans: NmConnection[]
  wifi: NmConnection[]
  wireguard: NmConnection[]
  firewalld: {
    available: boolean
    running: boolean
    default_zone: string
    zones: FirewalldZone[]
  }
  ovs?: {
    probed: boolean
    available: boolean
    bridges: Array<{ name: string; ports: string[] }>
    summary: string
  }
  summary: string
}

type SystemdUnitRow = {
  unit: string
  load: string
  active: string
  sub: string
  description: string
}

export type HostCockpitSystem = {
  probed: boolean
  kdump: { available: boolean; active: boolean; summary: string }
  selinux: { available: boolean; mode: string; enforce_supported: boolean }
  tuned: { available: boolean; active_profile: string; recommended_profile: string; profiles: string[] }
  realmd: { available: boolean; active: boolean; summary: string }
  systemd_failed: number
  systemd_units: SystemdUnitRow[]
  journal_errors_1h: number
  journal_recent: string[]
  packagekit?: { available: boolean; running: boolean; version: string; summary: string }
  summary: string
}

export type HostCockpitInventory = {
  host_id: string
  storage?: HostCockpitStorage | null
  network?: HostCockpitNetwork | null
  system?: HostCockpitSystem | null
}

export const getHostCockpitInventory = (hostId: string, section?: 'storage' | 'network' | 'system' | 'all') => {
  const q = section && section !== 'all' ? `?section=${section}` : ''
  return platformFetch<HostCockpitInventory>(`/api/v1/hosts/${hostId}/cockpit${q}`)
}

export const getClassicHostCockpitInventory = async (
  section?: 'storage' | 'network' | 'system' | 'all',
): Promise<HostCockpitInventory> => {
  const q = section && section !== 'all' ? `?section=${section}` : ''
  const res = await fetch(`${API}/host/cockpit${q}`, { credentials: 'include' })
  if (!res.ok) throw new Error(formatHttpErrorBody(res.status, res.statusText, await res.text()))
  const body = await res.json()
  return { host_id: 'local', ...body }
}

export const runClassicHostCockpitAction = async (
  action: string,
  payload: Record<string, unknown> = {},
) => {
  const res = await fetch(`${API}/host/cockpit/actions`, {
    method: 'POST',
    credentials: 'include',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action, payload }),
  })
  if (!res.ok) throw new Error(formatHttpErrorBody(res.status, res.statusText, await res.text()))
  return res.json() as Promise<{ status?: string; message?: string }>
}

export const runHostCockpitAction = (
  hostId: string,
  action: string,
  payload: Record<string, unknown> = {},
) =>
  platformFetch<{ status?: string; message?: string }>(`/api/v1/hosts/${hostId}/cockpit/actions`, {
    method: 'POST',
    body: JSON.stringify({ action, payload }),
  })
