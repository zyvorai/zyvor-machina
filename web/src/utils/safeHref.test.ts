// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { safeHref } from './safeHref'

describe('safeHref', () => {
  beforeEach(() => {
    vi.stubGlobal('window', { location: { origin: 'https://machina.test:5092' } })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('keeps http(s) and same-origin paths', () => {
    expect(safeHref('https://ctl.example:5093/api/v1/x')).toBe('https://ctl.example:5093/api/v1/x')
    expect(safeHref('/api/v1/platform/controller/api/v1/vms/a/viewer.vv')).toBe(
      'https://machina.test:5092/api/v1/platform/controller/api/v1/vms/a/viewer.vv',
    )
  })

  it('drops other schemes', () => {
    for (const bad of ['javascript:alert(1)', ' JavaScript:alert(1)', 'data:text/html,x', 'vbscript:x']) {
      expect(safeHref(bad)).toBeUndefined()
    }
  })
})
