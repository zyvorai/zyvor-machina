// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { formatHttpErrorBody, formatUserError } from '../utils/apiError'
import { redirectToLoginOnce } from './authRedirect'

const defaultOpts: RequestInit = { credentials: 'same-origin' }

/** fetch() throws TypeError / "NetworkError" when DNS fails, CORS blocks, TLS errors, or daemon is down. */
async function fetchApi(url: string, init?: RequestInit): Promise<Response> {
  let res: Response
  try {
    res = await fetch(url, init)
  } catch (e: unknown) {
    const msg = formatUserError(e)
    if (
      e instanceof TypeError
      || msg.includes('NetworkError')
      || msg.includes('Failed to fetch')
      || msg.includes('Load failed')
    ) {
      throw new Error(
        'Cannot reach the machina API (network error). Open the UI from the same URL as the daemon (host + port), ensure machina-daemon is running, and check HTTPS vs HTTP and any browser blockers.',
      )
    }
    throw e
  }
  // A 401 means the session expired / isn't authenticated. Redirect to login (debounced,
  // no-op on the login page) so a non-catching caller doesn't surface a raw "401 " as an
  // uncaught pageerror. The Response is still returned so callers that handle 401 (e.g.
  // getWsToken) keep working until navigation happens.
  if (res.status === 401) {
    redirectToLoginOnce()
  }
  return res
}

/**
 * Successful GET whose body must be JSON (stricter than `apiGet`, which can return plain text).
 * Used by {@link readJsonArray}, {@link readJsonObject}, etc.
 */
async function fetchJsonBody(url: string): Promise<unknown> {
  const res = await fetchApi(url, defaultOpts)
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, body))
  }
  const contentType = res.headers.get('content-type') || ''
  if (!contentType.includes('application/json')) {
    const text = await res.text().catch(() => '')
    const hint = text.trim().slice(0, 240)
    throw new Error(
      hint
        ? `Expected JSON from ${url} (${contentType || 'no Content-Type'}): ${hint}`
        : `Expected JSON from ${url} (${contentType || 'no Content-Type'})`,
    )
  }
  try {
    return await res.json()
  } catch {
    throw new Error(`Invalid JSON from ${url}`)
  }
}

/** JSON array responses (`GET /metrics`, `GET /k8s/nodes`, …). Wrong shape → `[]` + dev warning. */
export async function readJsonArray<T>(url: string): Promise<T[]> {
  const raw = await fetchJsonBody(url)
  if (!Array.isArray(raw)) {
    if (import.meta.env.DEV) {
      console.warn(`machina: expected JSON array from ${url}`)
    }
    return []
  }
  return raw as T[]
}

/** JSON object responses (maps, overview payloads, …). Wrong shape → `{}` + dev warning. */
export async function readJsonObject<T extends object>(url: string): Promise<T> {
  const raw = await fetchJsonBody(url)
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) {
    if (import.meta.env.DEV) {
      console.warn(`machina: expected JSON object from ${url}`)
    }
    return {} as T
  }
  return raw as T
}

/** Kubernetes-style `{ items: T[] }`. Missing/invalid `items` → `{ items: [] }`. */
export async function readJsonItemsList<T>(url: string): Promise<{ items: T[] }> {
  const raw = await fetchJsonBody(url)
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) {
    if (import.meta.env.DEV) {
      console.warn(`machina: expected JSON object with items from ${url}`)
    }
    return { items: [] }
  }
  const items = (raw as { items?: unknown }).items
  if (!Array.isArray(items)) {
    return { items: [] }
  }
  return { items: items as T[] }
}

/**
 * GET 2xx — body as plain text (libvirt XML, `/sysinfo`, …). Does not parse JSON.
 * Prefer {@link readJsonArray} / {@link readJsonObject} for JSON endpoints.
 */
export async function apiGetText(url: string): Promise<string> {
  const res = await fetchApi(url, defaultOpts)
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, body))
  }
  return res.text()
}

/**
 * Legacy GET: JSON when `Content-Type` is JSON, otherwise raw text typed as `T`.
 * Prefer {@link readJsonArray}, {@link readJsonObject}, {@link readJsonItemsList}, or {@link apiGetText}.
 * Kept for ad-hoc callers outside the API modules.
 */
export async function apiGet<T>(url: string): Promise<T> {
  const res = await fetchApi(url, defaultOpts)
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, body))
  }
  const contentType = res.headers.get('content-type') || ''
  if (contentType.includes('application/json')) {
    return res.json()
  }
  return await res.text() as T
}

/** GET binary (screenshots, downloads). */
export async function apiGetBlob(url: string): Promise<Blob> {
  const res = await fetchApi(url, defaultOpts)
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, body))
  }
  return res.blob()
}

export async function apiPost<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetchApi(url, {
    ...defaultOpts,
    method: 'POST',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, text))
  }
  const contentType = res.headers.get('content-type') || ''
  if (contentType.includes('application/json')) {
    return res.json()
  }
  return await res.text() as T
}

export async function apiPostVoid(url: string, body?: unknown): Promise<void> {
  const res = await fetchApi(url, {
    ...defaultOpts,
    method: 'POST',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, text))
  }
}

export async function apiPut<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetchApi(url, {
    ...defaultOpts,
    method: 'PUT',
    headers: body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, text))
  }
  const contentType = res.headers.get('content-type') || ''
  if (contentType.includes('application/json')) {
    return res.json()
  }
  return await res.text() as T
}

export async function apiDelete(url: string): Promise<void> {
  const res = await fetchApi(url, { ...defaultOpts, method: 'DELETE' })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, text))
  }
}

/** DELETE that returns the JSON body (`null` when there is none). */
export async function apiDeleteJson<T>(url: string): Promise<T | null> {
  const res = await fetchApi(url, { ...defaultOpts, method: 'DELETE' })
  if (!res.ok) {
    const text = await res.text().catch(() => '')
    throw new Error(formatHttpErrorBody(res.status, res.statusText, text))
  }
  const contentType = res.headers.get('content-type') || ''
  return contentType.includes('application/json') ? res.json() : null
}

function wsAuthHeaders(): HeadersInit {
  const h = new Headers()
  try {
    const jwt = localStorage.getItem('machina_platform_jwt')
    if (jwt) {
      h.set('Authorization', `Bearer ${jwt}`)
      return h
    }
  } catch {
    /* private browsing / blocked storage */
  }
  return h
}

export async function getWsToken(): Promise<string> {
  const res = await fetchApi('/api/v1/ws-token', {
    method: 'POST',
    credentials: 'same-origin',
    cache: 'no-store',
    headers: wsAuthHeaders(),
  })
  if (res.status === 401) {
    throw new Error('Session expired — sign in again at /login to use Shell or Serial consoles.')
  }
  if (!res.ok) throw new Error('Failed to get WebSocket token')
  let data: unknown
  try {
    data = await res.json()
  } catch {
    throw new Error('Failed to get WebSocket token')
  }
  const tok =
    data !== null && typeof data === 'object' && !Array.isArray(data) && 'token' in data
      ? (data as { token?: unknown }).token
      : undefined
  if (typeof tok !== 'string' || !tok) {
    throw new Error('Failed to get WebSocket token')
  }
  return tok
}
