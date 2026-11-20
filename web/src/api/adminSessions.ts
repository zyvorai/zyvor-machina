// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { parseResponseError } from './parseResponseError'

const API = '/api/v1'

interface AdminSessionRow {
  session_id: string
  username: string
  age_secs: number
  expires_in_secs: number
  is_current: boolean
}

export interface AdminSessionsResponse {
  sessions: AdminSessionRow[]
  total_sessions: number
  users_logged_in: number
  sessions_per_username: Record<string, number>
}

function normalizeAdminSessionRow(raw: unknown): AdminSessionRow | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const o = raw as Record<string, unknown>
  const sid = o.session_id
  if (typeof sid !== 'string' || !sid.trim()) return null
  const age = typeof o.age_secs === 'number' && Number.isFinite(o.age_secs) ? Math.max(0, Math.floor(o.age_secs)) : 0
  const exp =
    typeof o.expires_in_secs === 'number' && Number.isFinite(o.expires_in_secs)
      ? Math.max(0, Math.floor(o.expires_in_secs))
      : 0
  return {
    session_id: sid,
    username: typeof o.username === 'string' ? o.username : String(o.username ?? ''),
    age_secs: age,
    expires_in_secs: exp,
    is_current: Boolean(o.is_current),
  }
}

function normalizeAdminSessionsResponse(raw: unknown): AdminSessionsResponse {
  const empty: AdminSessionsResponse = {
    sessions: [],
    total_sessions: 0,
    users_logged_in: 0,
    sessions_per_username: {},
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return empty
  const o = raw as Record<string, unknown>
  const sessionsIn = o.sessions
  const sessions: AdminSessionRow[] = []
  if (Array.isArray(sessionsIn)) {
    for (const row of sessionsIn) {
      const n = normalizeAdminSessionRow(row)
      if (n) sessions.push(n)
    }
  }
  const total =
    typeof o.total_sessions === 'number' && Number.isFinite(o.total_sessions)
      ? Math.max(0, Math.floor(o.total_sessions))
      : sessions.length
  const users =
    typeof o.users_logged_in === 'number' && Number.isFinite(o.users_logged_in)
      ? Math.max(0, Math.floor(o.users_logged_in))
      : 0
  let perUser: Record<string, number> = {}
  const spu = o.sessions_per_username
  if (spu !== null && typeof spu === 'object' && !Array.isArray(spu)) {
    perUser = {}
    for (const [k, v] of Object.entries(spu)) {
      if (typeof v === 'number' && Number.isFinite(v)) perUser[k] = v
      else if (v != null && Number.isFinite(Number(v))) perUser[k] = Number(v)
    }
  }
  return {
    sessions,
    total_sessions: total,
    users_logged_in: users,
    sessions_per_username: perUser,
  }
}

export async function listAdminSessions(): Promise<AdminSessionsResponse> {
  const res = await fetch(`${API}/admin/sessions`, { credentials: 'same-origin' })
  if (!res.ok) throw await parseResponseError(res)
  let raw: unknown
  try {
    raw = await res.json()
  } catch {
    return normalizeAdminSessionsResponse(null)
  }
  return normalizeAdminSessionsResponse(raw)
}

export async function revokeAdminSession(sessionId: string): Promise<void> {
  const res = await fetch(`${API}/admin/sessions/${encodeURIComponent(sessionId)}`, {
    method: 'DELETE',
    credentials: 'same-origin',
  })
  if (!res.ok) throw await parseResponseError(res)
}
