// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Centralized handling for a *persistent* 401 (session truly expired / not authenticated).
// Without this, a 401 on any secondary API call bubbles up as a raw "401 " Error and, if a
// caller doesn't catch it, surfaces as an uncaught pageerror / "Application error" screen.
// Instead we send the user to the login page once (preserving where they were), which is
// the correct UX for an expired session.

let scheduled = false

/**
 * Resolve a post-login destination from `?next=` (or an explicit value).
 * Only same-origin relative paths are accepted — blocks open redirects (`//evil`, `https://…`).
 */
export function safePostLoginPath(next: string | null | undefined, fallback = '/'): string {
  const raw = (next ?? '').trim()
  if (!raw.startsWith('/') || raw.startsWith('//') || raw.includes('://')) {
    return fallback
  }
  return raw
}

/**
 * Navigate to the login page on a persistent 401. Debounced (fires once) and a no-op when
 * already on the login page, so concurrent failing requests don't loop or thrash history.
 * Callers should invoke this only AFTER any token-refresh/retry has also failed.
 */
export function redirectToLoginOnce(): void {
  if (typeof window === 'undefined') return
  const path = window.location.pathname
  if (path === '/login' || path.startsWith('/login')) return
  if (scheduled) return
  scheduled = true
  const next = encodeURIComponent(window.location.pathname + window.location.search)
  window.location.assign(`/login?next=${next}`)
}
