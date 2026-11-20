// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native compute flavor catalog (controller::api::flavors) — Phase 1 of the
// external-cloud-client replacement. Goes through the platform controller proxy; the
// daemon's external-cloud-client integration has since been fully removed.

import { platformFetch } from './platform'

export interface NativeFlavor {
  id: string
  name: string
  vcpus: number
  memory_mib: number
  disk_gib: number
  description: string
  is_public: boolean
}

export function listFlavors(): Promise<NativeFlavor[]> {
  return platformFetch<NativeFlavor[]>('/api/v1/flavors')
}

export function getFlavor(id: string): Promise<NativeFlavor> {
  return platformFetch<NativeFlavor>(`/api/v1/flavors/${encodeURIComponent(id)}`)
}

export function createFlavor(body: {
  name: string
  vcpus: number
  memory_mib: number
  disk_gib: number
  description?: string
  is_public?: boolean
}): Promise<NativeFlavor> {
  return platformFetch<NativeFlavor>('/api/v1/flavors', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteFlavor(id: string): Promise<void> {
  await platformFetch<{ deleted: boolean }>(`/api/v1/flavors/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
}
