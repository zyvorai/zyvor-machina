// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import {
  formatDateTime,
  formatNumber,
  formatRelative,
  isRtl,
  pickLang,
} from './localeFormat'

describe('localeFormat', () => {
  it('picks the first supported language and falls back to English', () => {
    expect(pickLang(['fr-FR', 'es-MX', 'en'])).toBe('es')
    expect(pickLang(['de'])).toBe('en')
    expect(pickLang([])).toBe('en')
  })

  it('knows right-to-left languages', () => {
    expect(isRtl('ar')).toBe(true)
    expect(isRtl('he-IL')).toBe(true)
    expect(isRtl('es')).toBe(false)
  })

  it('formats numbers per locale and survives non-finite input', () => {
    expect(formatNumber(1234567.5, undefined, 'en')).toBe('1,234,567.5')
    expect(formatNumber(1234567.5, undefined, 'de')).toBe('1.234.567,5')
    expect(formatNumber(Number.NaN)).toBe('—')
  })

  it('formats dates in the requested time zone and rejects bad input', () => {
    const iso = '2026-10-12T09:30:00Z'
    expect(
      formatDateTime(iso, { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }, 'en'),
    ).toContain('Oct 12, 2026')
    expect(formatDateTime('not a date')).toBe('—')
    expect(formatDateTime(null)).toBe('—')
  })

  it('formats relative times', () => {
    const now = Date.parse('2026-10-12T12:00:00Z')
    expect(formatRelative('2026-10-12T11:57:00Z', now, 'en')).toBe('3 minutes ago')
    expect(formatRelative('2026-10-14T12:00:00Z', now, 'en')).toBe('in 2 days')
    expect(formatRelative(undefined, now, 'en')).toBe('—')
  })
})
