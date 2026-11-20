// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native ports (controller::api::networking) — VM NIC bindings, the Neutron-port
// equivalent. Goes through the platform controller proxy.

import { platformFetch } from './platform'

export interface NativePort {
  id: string
  network_id: string
  project_id: string | null
  vm_id: string | null
  mac_address: string | null
  security_group_id: string | null
  status: string
}

export function listPorts(): Promise<NativePort[]> {
  return platformFetch<NativePort[]>('/api/v1/ports')
}

export function createPort(body: { network_id: string; vm_id?: string; security_group_id?: string }): Promise<NativePort> {
  return platformFetch<NativePort>('/api/v1/ports', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deletePort(id: string): Promise<void> {
  await platformFetch(`/api/v1/ports/${encodeURIComponent(id)}`, { method: 'DELETE' })
}
