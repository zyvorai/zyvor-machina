// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// One place for locale-aware dates, numbers and relative times, and for the document's
// `lang`/`dir`. Pages used to call toLocale*() directly (about 116 call sites); new code
// should use these so the operator's language, not the browser default, decides.

export const SUPPORTED_LANGS = ['en', 'es'] as const
export type SupportedLang = (typeof SUPPORTED_LANGS)[number]

const RTL = new Set(['ar', 'he', 'fa', 'ur', 'ps', 'sd', 'yi', 'dv', 'ku'])
const STORAGE_KEY = 'machina_lang'

export function isRtl(lang: string): boolean {
  return RTL.has(lang.toLowerCase().split(/[-_]/)[0])
}

/** The first supported language in a BCP-47 list (`es-MX` matches `es`), else `en`. */
export function pickLang(candidates: readonly string[]): SupportedLang {
  for (const c of candidates) {
    const base = c.toLowerCase().split(/[-_]/)[0]
    const hit = SUPPORTED_LANGS.find((l) => l === base)
    if (hit) return hit
  }
  return 'en'
}

/** Stored choice, else the browser's language list, else English. */
export function detectLang(): SupportedLang {
  let stored: string | null = null
  try {
    stored = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null
  } catch {
    stored = null
  }
  if (stored) return pickLang([stored])
  const nav = typeof navigator !== 'undefined' ? (navigator.languages?.length ? navigator.languages : [navigator.language]) : []
  return pickLang(nav.filter(Boolean))
}

/** Keeps `<html lang dir>` in step with the UI language (screen readers, RTL layout). */
export function applyDocumentLocale(lang: string): void {
  if (typeof document === 'undefined') return
  document.documentElement.lang = lang
  document.documentElement.dir = isRtl(lang) ? 'rtl' : 'ltr'
}

let current: string = 'en'

export function setFormatLocale(lang: string): void {
  current = lang
}

function loc(locale?: string): string {
  return locale ?? current
}

export function formatNumber(n: number, opts?: Intl.NumberFormatOptions, locale?: string): string {
  if (!Number.isFinite(n)) return '—'
  return new Intl.NumberFormat(loc(locale), opts).format(n)
}

export function formatDateTime(
  value: string | number | Date | null | undefined,
  opts: Intl.DateTimeFormatOptions = { dateStyle: 'medium', timeStyle: 'short' },
  locale?: string,
): string {
  if (value === null || value === undefined || value === '') return '—'
  const d = value instanceof Date ? value : new Date(value)
  if (Number.isNaN(d.getTime())) return '—'
  return new Intl.DateTimeFormat(loc(locale), opts).format(d)
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
  ['second', 1],
]

/** "3 minutes ago" / "in 2 days", in the UI language. */
export function formatRelative(
  value: string | number | Date | null | undefined,
  now: number = Date.now(),
  locale?: string,
): string {
  if (value === null || value === undefined || value === '') return '—'
  const t = (value instanceof Date ? value : new Date(value)).getTime()
  if (Number.isNaN(t)) return '—'
  const diff = Math.round((t - now) / 1000)
  const abs = Math.abs(diff)
  const rtf = new Intl.RelativeTimeFormat(loc(locale), { numeric: 'auto' })
  for (const [unit, secs] of UNITS) {
    if (abs >= secs || unit === 'second') {
      return rtf.format(Math.round(diff / secs), unit)
    }
  }
  return rtf.format(0, 'second')
}
