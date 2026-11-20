// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

/** EC2-style key/value tags on any taggable resource. Mirrors the controller's validation (`api/tags.rs`). */
export type TagMap = Record<string, string>

export type TagResourceType =
  | 'vm' | 'volume' | 'snapshot' | 'security_group' | 'keypair' | 'image' | 'port'
  | 'vpc' | 'subnet' | 'instance_group' | 'launch_template'

export interface TagsResponse {
  resource_type: TagResourceType
  id: string
  /** EC2-style id, e.g. i-0123456789abcdef0 */
  ec2_id: string
  tags: TagMap
}

export const MAX_TAGS = 50
const KEY_RE = /^[\p{L}\p{N} _.:/=+\-@]+$/u

/** Null when the key is acceptable, otherwise the reason (same rules the server applies). */
export function validateTagKey(key: string): string | null {
  if (!key) return 'Enter a key.'
  if ([...key].length > 128) return 'Keys are at most 128 characters.'
  if (!KEY_RE.test(key)) return 'Keys may use letters, digits and _ . : / = + - @ and spaces.'
  if (/^(aws|machina):/i.test(key)) return 'Keys starting with aws: or machina: are reserved.'
  return null
}

export function validateTagValue(value: string): string | null {
  if ([...value].length > 256) return 'Values are at most 256 characters.'
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(value)) return 'Values may not contain control characters.'
  return null
}

const path = (type: string, id: string) => `/api/v1/tags/${encodeURIComponent(type)}/${encodeURIComponent(id)}`

export const getTags = (type: TagResourceType, id: string) => platformFetch<TagsResponse>(path(type, id))

export const putTags = (type: TagResourceType, id: string, tags: TagMap) =>
  platformFetch<TagsResponse>(path(type, id), { method: 'PUT', body: JSON.stringify({ tags }) })

export const deleteTags = (type: TagResourceType, id: string, keys: string[]) =>
  platformFetch<TagsResponse>(path(type, id), { method: 'DELETE', body: JSON.stringify({ keys }) })

export interface TagRow { resource_type: string; id: string; ec2_id: string | null; key: string; value: string }

/** Find resources by tag (EC2's DescribeTags). */
export function listTags(q: { type?: TagResourceType; key?: string; value?: string } = {}) {
  const p = new URLSearchParams()
  if (q.type) p.set('type', q.type)
  if (q.key) p.set('key', q.key)
  if (q.value) p.set('value', q.value)
  const s = p.toString()
  return platformFetch<TagRow[]>(`/api/v1/tags${s ? `?${s}` : ''}`)
}
