// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { parseResponseError } from './parseResponseError'

const API = '/api/v1'

/** RBAC role from session or API token (daemon `roles.json` / token metadata). */
export type SessionRole = 'admin' | 'operator' | 'readonly'
type SessionAuthSource = 'pam' | 'ldap' | 'oidc' | 'api_token'

export interface AuthSession {
  authenticated: boolean
  username?: string
  effective_linux_user?: string | null
  /** Opaque id for this browser tab session (root-only admin UI). */
  session_id?: string | null
  /** Present when authenticated via cookie or token-aware session. */
  role?: SessionRole
  auth_source?: SessionAuthSource
  /** Concurrent browser sessions for this username (daemon in-memory store). */
  active_sessions_for_user?: number
  /** `0` means unlimited concurrent sessions per username. */
  max_sessions_per_user?: number
}

export interface AuthProviders {
  pam: { enabled: boolean }
  ldap: { enabled: boolean }
  oidc: { enabled: boolean; button_label: string }
  saml?: { enabled: boolean; button_label: string; login_available?: boolean }
}

const DEFAULT_PROVIDERS: AuthProviders = {
  pam: { enabled: true },
  ldap: { enabled: false },
  oidc: { enabled: false, button_label: 'Sign in with SSO' },
  saml: { enabled: false, button_label: 'Sign in with SAML', login_available: false },
}

/** Map daemon JSON (OIDC/PAM) to a known role; unknown shapes become `undefined` so callers can apply `?? fallback`. */
function parseSessionRole(value: unknown): SessionRole | undefined {
  if (value === 'admin' || value === 'operator' || value === 'readonly') return value
  if (typeof value !== 'string') return undefined
  const x = value.trim().toLowerCase()
  if (x === 'admin' || x === 'operator' || x === 'readonly') return x as SessionRole
  return undefined
}

function parseAuthSource(value: unknown): SessionAuthSource | undefined {
  if (value === 'pam' || value === 'ldap' || value === 'oidc' || value === 'api_token') return value
  if (typeof value !== 'string') return undefined
  const x = value.trim().toLowerCase()
  if (x === 'pam' || x === 'ldap' || x === 'oidc' || x === 'api_token') return x as SessionAuthSource
  return undefined
}

/**
 * Normalize `/auth/session` JSON so UI never crashes on missing fields (proxies, partial responses, OIDC edge cases).
 */
function normalizeAuthSession(raw: unknown): AuthSession {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    return { authenticated: false }
  }
  const o = raw as Record<string, unknown>
  if (o.authenticated !== true) {
    return { authenticated: false }
  }

  const username = o.username == null ? '' : String(o.username)
  const eff = o.effective_linux_user
  const effective_linux_user =
    eff === undefined || eff === null || eff === ''
      ? null
      : String(eff)

  const sid = o.session_id
  let session_id: string | null | undefined
  if (sid === undefined || sid === null) session_id = sid as undefined | null
  else if (typeof sid === 'string') session_id = sid
  else session_id = String(sid)

  const active_sessions_for_user =
    typeof o.active_sessions_for_user === 'number' ? o.active_sessions_for_user : undefined
  const max_sessions_per_user =
    typeof o.max_sessions_per_user === 'number' ? o.max_sessions_per_user : undefined

  return {
    authenticated: true,
    username,
    effective_linux_user,
    session_id,
    role: parseSessionRole(o.role),
    auth_source: parseAuthSource(o.auth_source),
    active_sessions_for_user,
    max_sessions_per_user,
  }
}

function normalizeAuthProviders(raw: unknown): AuthProviders {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return DEFAULT_PROVIDERS
  const o = raw as Record<string, unknown>
  const pamIn = o.pam
  const ldapIn = o.ldap
  const oidcIn = o.oidc

  const ldapEnabled =
    typeof ldapIn === 'object' && ldapIn !== null && 'enabled' in ldapIn
      ? Boolean((ldapIn as { enabled?: unknown }).enabled)
      : false

  const pamEnabled =
    typeof pamIn === 'object' && pamIn !== null && 'enabled' in pamIn
      ? Boolean((pamIn as { enabled?: unknown }).enabled)
      : !ldapEnabled

  let oidcEnabled = false
  let buttonLabel = DEFAULT_PROVIDERS.oidc.button_label
  if (typeof oidcIn === 'object' && oidcIn !== null) {
    oidcEnabled = Boolean((oidcIn as { enabled?: unknown }).enabled)
    const bl = (oidcIn as { button_label?: unknown }).button_label
    if (typeof bl === 'string' && bl.trim()) buttonLabel = bl
  }

  const samlIn = o.saml
  let samlEnabled = false
  let samlLabel = DEFAULT_PROVIDERS.saml?.button_label ?? 'Sign in with SAML'
  let samlLoginAvailable = false
  if (typeof samlIn === 'object' && samlIn !== null) {
    samlEnabled = Boolean((samlIn as { enabled?: unknown }).enabled)
    const bl = (samlIn as { button_label?: unknown }).button_label
    if (typeof bl === 'string' && bl.trim()) samlLabel = bl
    samlLoginAvailable = Boolean((samlIn as { login_available?: unknown }).login_available)
  }

  return {
    pam: { enabled: pamEnabled },
    ldap: { enabled: ldapEnabled },
    oidc: { enabled: oidcEnabled, button_label: buttonLabel },
    saml: { enabled: samlEnabled, button_label: samlLabel, login_available: samlLoginAvailable },
  }
}

export async function login(username: string, password: string): Promise<{ status: string; username: string }> {
  const res = await fetch(`${API}/auth/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
    credentials: 'same-origin',
  })
  if (!res.ok) {
    throw await parseResponseError(res)
  }
  let raw: unknown
  try {
    raw = await res.json()
  } catch {
    return { status: 'ok', username }
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
    return { status: 'ok', username }
  }
  const ro = raw as Record<string, unknown>
  const status = typeof ro.status === 'string' && ro.status ? ro.status : 'ok'
  const u = typeof ro.username === 'string' ? ro.username : username
  return { status, username: u }
}

export async function logout(): Promise<void> {
  await fetch(`${API}/auth/logout`, {
    method: 'POST',
    credentials: 'same-origin',
  })
}

export async function getSession(): Promise<AuthSession> {
  const res = await fetch(`${API}/auth/session`, { credentials: 'same-origin' })
  if (!res.ok) return { authenticated: false }
  let raw: unknown
  try {
    raw = await res.json()
  } catch {
    return { authenticated: false }
  }
  return normalizeAuthSession(raw)
}

export async function getAuthProviders(): Promise<AuthProviders> {
  const res = await fetch(`${API}/auth/providers`, { credentials: 'same-origin' })
  if (!res.ok) return DEFAULT_PROVIDERS
  let raw: unknown
  try {
    raw = await res.json()
  } catch {
    return DEFAULT_PROVIDERS
  }
  return normalizeAuthProviders(raw)
}

export function beginOidcLogin(): void {
  window.location.assign(`${API}/auth/oidc/login`)
}

/** Exchange platform JWT from `?token=` deep link for a browser session cookie. */
export async function exchangeTokenForSession(token: string): Promise<{ username: string }> {
  const res = await fetch(`${API}/auth/token/session`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify({ token }),
  })
  if (!res.ok) throw await parseResponseError(res)
  const body = (await res.json()) as { username?: string }
  return { username: body.username ?? '' }
}
