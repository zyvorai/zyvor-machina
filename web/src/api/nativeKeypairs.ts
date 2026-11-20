// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Native SSH keypair catalog (controller::api::keypairs) — Phase 4 of the
// external-cloud-client replacement. Only the public key is ever stored — no
// private-key generation/download, unlike Nova's "create keypair" flow.

import { platformFetch } from './platform'

export interface NativeKeypair {
  id: string
  project_id: string | null
  name: string
  public_key: string
  fingerprint: string
}

export function listKeypairs(): Promise<NativeKeypair[]> {
  return platformFetch<NativeKeypair[]>('/api/v1/keypairs')
}

export function createKeypair(body: { name: string; public_key: string }): Promise<NativeKeypair> {
  return platformFetch<NativeKeypair>('/api/v1/keypairs', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function deleteKeypair(id: string): Promise<void> {
  await platformFetch(`/api/v1/keypairs/${encodeURIComponent(id)}`, { method: 'DELETE' })
}
