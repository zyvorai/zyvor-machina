// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { platformFetch } from './platform'

export interface ContentImage {
  id: string
  name: string
  kind: string
  path: string
  size_gib: number
  status: string
  category?: string
  description?: string
  submitted_by?: string | null
  approved_by?: string | null
  approved_at?: string | null
  rejected_reason?: string | null
  created_at: string
}

export const listContentImages = (params?: { status?: string }) => {
  const q = params?.status ? `?status=${encodeURIComponent(params.status)}` : ''
  return platformFetch<ContentImage[]>(`/api/v1/content/images${q}`)
}

export const createContentImage = (body: {
  name: string
  kind?: string
  path: string
  size_gib?: number
  category?: string
  description?: string
}) => platformFetch<ContentImage>('/api/v1/content/images', { method: 'POST', body: JSON.stringify(body) })

export const approveContentImage = (id: string) =>
  platformFetch<ContentImage>(`/api/v1/content/images/${id}/approve`, { method: 'POST', body: '{}' })

export const rejectContentImage = (id: string, reason?: string) =>
  platformFetch<ContentImage>(`/api/v1/content/images/${id}/reject`, {
    method: 'POST',
    body: JSON.stringify({ reason }),
  })
