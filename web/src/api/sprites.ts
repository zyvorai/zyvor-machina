// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { readJsonArray, apiPost, apiDelete } from './client'

const API = '/api/v1'

type SpriteState = 'booting' | 'running' | 'paused' | 'suspended' | 'reaping' | 'gone'
export type SpriteBackend = 'libvirt' | 'cloudhypervisor' | 'firecracker'

export interface SpriteHandle {
  sprite_id: string
  state: SpriteState
  /** RFC3339 */
  created_at: string
  /** RFC3339. Meaning depends on `state` — a "suspended" sprite's is a disk-retention deadline, not the original TTL. */
  expires_at: string
  vsock_cid?: number
  backend: SpriteBackend
  /** Attached to the host's "default" NAT network (virbr0) for outbound-only internet access. */
  network_egress: boolean
  vcpus: number
  memory_mb: number
  /** RFC3339. Present only while `state === 'suspended'`. */
  suspended_at?: string
}

interface SpriteCreateRequest {
  golden_image: string
  vcpus?: number
  memory_mb?: number
  ttl_seconds?: number
  backend?: SpriteBackend
  network_egress?: boolean
}

interface SpriteRestoreRequest {
  ttl_seconds?: number
}

export const listSprites = () => readJsonArray<SpriteHandle>(`${API}/sprites`)
export const createSprite = (body: SpriteCreateRequest) => apiPost<SpriteHandle>(`${API}/sprites`, body)
export const deleteSprite = (id: string) => apiDelete(`${API}/sprites/${encodeURIComponent(id)}`)

/** Pause a running sprite in place — process stays alive, still counts against its original TTL. */
export const pauseSprite = (id: string) => apiPost<SpriteHandle>(`${API}/sprites/${encodeURIComponent(id)}/pause`, {})
/** Resume a paused sprite. */
export const resumeSprite = (id: string) => apiPost<SpriteHandle>(`${API}/sprites/${encodeURIComponent(id)}/resume`, {})
/** Snapshot to disk and kill the process ("suspend"). */
export const snapshotSprite = (id: string) => apiPost<SpriteHandle>(`${API}/sprites/${encodeURIComponent(id)}/snapshot`, {})
/** Restore a suspended sprite from its snapshot. */
export const restoreSprite = (id: string, body: SpriteRestoreRequest = {}) =>
  apiPost<SpriteHandle>(`${API}/sprites/${encodeURIComponent(id)}/restore`, body)

/** Golden-image registry keys available to boot a sprite from (bare names, no `.qcow2`). */
export const listSpriteGoldenImages = () => readJsonArray<string>(`${API}/sprites/golden-images`)
