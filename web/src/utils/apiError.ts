// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Stable daemon `error_code` values → short user-facing labels. */
const API_ERROR_LABELS: Record<string, string> = {
  operation_failed: 'The operation failed on the server',
  not_found: 'The requested resource was not found',
  invalid_request: 'The request was invalid',
  forbidden: 'You do not have permission for this action',
  libvirt_connection: 'Could not connect to libvirt on the host',
  internal_error: 'An internal server error occurred',
  unauthorized: 'Authentication required or your session expired',
  conflict: 'That name is already in use',
  invalid_name: 'Invalid name — use letters, numbers, spaces, and common punctuation',
}
const ERROR_CODE_RE =
  /\((operation_failed|not_found|invalid_request|forbidden|libvirt_connection|internal_error|unauthorized|conflict|invalid_name)\)\s*$/i

/** Human label for a stable API error code. */
export function friendlyErrorCode(code: string): string {
  return API_ERROR_LABELS[code] ?? code.replace(/_/g, ' ')
}

/**
 * The daemon and controller answer failures as `{"error":"…"}` (or `{"message":"…"}`). When such an
 * envelope reaches the UI as an Error message (e.g. a caller threw `await res.text()`), show the
 * message, not the JSON. Anything that is not a recognisable envelope is returned unchanged.
 */
export function unwrapJsonEnvelope(text: string): string {
  const t = text.trim()
  if (!t.startsWith('{')) return text
  try {
    const j = JSON.parse(t) as { error?: unknown; message?: unknown }
    const msg = typeof j.error === 'string' ? j.error : typeof j.message === 'string' ? j.message : ''
    return msg.trim() ? msg.trim() : text
  } catch {
    return text
  }
}

/** Strip HTML pages and trim noisy API text for UI display. */
export function sanitizeErrorText(text: string): string {
  const t = unwrapJsonEnvelope(text).trim()
  if (!t) return ''

  if (/<!DOCTYPE\s+html/i.test(t) || /<html[\s>]/i.test(t) || t.includes('</html>')) {
    return (
      'The API returned an HTML error page instead of JSON. ' +
      'This usually means a reverse proxy failure or machina-daemon is down.'
    )
  }

  if (t.includes('<') && t.includes('>')) {
    const stripped = t.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim()
    if (stripped.length > 0 && stripped.length < t.length * 0.6) {
      return stripped.length > 500 ? `${stripped.slice(0, 497)}…` : stripped
    }
  }

  return t.length > 600 ? `${t.slice(0, 597)}…` : t
}

/**
 * Build a user-facing message from an HTTP error body (JSON `{ error, error_code }` or plain/HTML).
 */
export function formatHttpErrorBody(status: number, statusText: string, text: string): string {
  const raw = text.trim()
  const statusLabel = `HTTP ${status}${statusText ? ` ${statusText}` : ''}`

  if (!raw) {
    return `Request failed (${statusLabel})`
  }

  if (/<!DOCTYPE\s+html/i.test(raw) || /^<\s*html/i.test(raw)) {
    return (
      `Request failed (${statusLabel}): the server returned an HTML error page instead of JSON. ` +
      'Check that machina-daemon is running and reachable.'
    )
  }

  try {
    const j = JSON.parse(raw) as { error?: string; message?: string; error_code?: string; remediation?: string }
    const code = typeof j.error_code === 'string' ? j.error_code : undefined
    const remediation = typeof j.remediation === 'string' ? j.remediation : undefined
    const rawMsg =
      typeof j.error === 'string' ? j.error : typeof j.message === 'string' ? j.message : ''
    const clean = sanitizeErrorText(rawMsg)

    if (clean) {
      if (code && (clean === code || clean === friendlyErrorCode(code))) {
        return remediation ? `${friendlyErrorCode(code)} — ${remediation}` : friendlyErrorCode(code)
      }
      if (code && !clean.toLowerCase().includes(code.replace(/_/g, ' '))) {
        const base = `${clean} (${friendlyErrorCode(code)})`
        return remediation ? `${base} — ${remediation}` : base
      }
      return remediation ? `${clean} — ${remediation}` : clean
    }
    if (code) {
      return friendlyErrorCode(code)
    }
  } catch {
    /* not JSON */
  }

  const sanitized = sanitizeErrorText(raw)
  if (sanitized !== raw) {
    return sanitized
  }
  if (raw.length > 400) {
    return `Request failed (${statusLabel}): ${raw.slice(0, 200)}…`
  }
  return `Request failed (${statusLabel}): ${raw}`
}

/** Format any thrown value for toasts and banners. */
/** True when the platform controller returned 404 / not_found for a VM or other resource. */
export function isPlatformNotFoundError(e: unknown): boolean {
  if (e && typeof e === 'object' && 'error_code' in e) {
    const code = (e as { error_code?: string }).error_code
    if (code === 'not_found') return true
  }
  const msg = formatUserError(e).toLowerCase()
  return /\bnot[_ ]found\b/.test(msg)
}

export function formatUserError(e: unknown): string {
  if (e && typeof e === 'object' && 'remediation' in e) {
    const remediation = (e as { remediation?: string }).remediation
    const base = e instanceof Error ? sanitizeErrorText(e.message) : ''
    if (remediation && base) return `${base} — ${remediation}`
    if (remediation) return remediation
  }
  if (e instanceof Error) {
    const msg = sanitizeErrorText(e.message)
    const m = msg.match(ERROR_CODE_RE)
    if (m?.[1] && msg.replace(ERROR_CODE_RE, '').trim().length < 8) {
      return friendlyErrorCode(m[1])
    }
    return msg || 'Unknown error'
  }
  return sanitizeErrorText(String(e)) || 'Unknown error'
}
