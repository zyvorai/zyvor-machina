// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native libvirt network catalog (controller::api::networks) — used for the
// network picker on native "instance" NIC attach. Goes through the platform
// controller proxy.

import { platformFetch } from './platform'

export interface NativeNetwork {
  id: string
  name: string
  backend: string
  vlan_id: number | null
  bridge: string | null
  segment_id: string | null
}

export function listNetworks(): Promise<NativeNetwork[]> {
  return platformFetch<NativeNetwork[]>('/api/v1/networks')
}

export function createNetwork(body: { name: string; backend?: string; vlan_id?: number; bridge?: string }): Promise<NativeNetwork> {
  return platformFetch<NativeNetwork>('/api/v1/networks', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteNetwork(id: string): Promise<void> {
  await platformFetch(`/api/v1/networks/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

export function getNetwork(id: string): Promise<NativeNetwork> {
  return platformFetch<NativeNetwork>(`/api/v1/networks/${encodeURIComponent(id)}`)
}
