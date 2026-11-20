// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { safePostLoginPath } from './authRedirect'

describe('safePostLoginPath', () => {
  it('accepts same-origin relative paths', () => {
    expect(safePostLoginPath('/platform/vms/abc?tab=backup')).toBe('/platform/vms/abc?tab=backup')
    expect(safePostLoginPath('/vms/foo')).toBe('/vms/foo')
  })

  it('rejects open redirects', () => {
    expect(safePostLoginPath('//evil.example')).toBe('/')
    expect(safePostLoginPath('https://evil.example')).toBe('/')
    expect(safePostLoginPath('http://evil.example/x')).toBe('/')
  })

  it('falls back when missing or empty', () => {
    expect(safePostLoginPath(null)).toBe('/')
    expect(safePostLoginPath('')).toBe('/')
    expect(safePostLoginPath('  ', '/platform')).toBe('/platform')
  })
})
