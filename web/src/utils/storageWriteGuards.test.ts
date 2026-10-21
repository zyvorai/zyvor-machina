// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { afterEach, describe, expect, it, vi } from 'vitest'
import { addRecentVM, removeRecentVMs } from './recentVMs'
import { saveFinderInspectorWidth } from './finderInspectorWidth'

// localStorage.setItem throws in Safari private mode and when the quota is full.
function throwingStorage() {
  vi.stubGlobal('localStorage', {
    getItem: () => null,
    setItem: () => { throw new DOMException('quota', 'QuotaExceededError') },
    removeItem: () => {},
  })
}

describe('localStorage write guards', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('addRecentVM does not throw when storage rejects writes', () => {
    throwingStorage()
    expect(() => addRecentVM('vm1')).not.toThrow()
  })

  it('removeRecentVMs does not throw when storage rejects writes', () => {
    throwingStorage()
    expect(() => removeRecentVMs(['vm1'])).not.toThrow()
  })

  it('saveFinderInspectorWidth does not throw when storage rejects writes', () => {
    throwingStorage()
    expect(() => saveFinderInspectorWidth(300)).not.toThrow()
  })
})
