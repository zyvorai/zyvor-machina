// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, it, expect, beforeEach, vi } from 'vitest'
import {
  loadPlatformDesktopTier,
  savePlatformDesktopTier,
  PLATFORM_DESKTOP_TIER_KEY,
} from '../utils/platformDesktopTier'

const makeLocalStorageMock = () => {
  let store: Record<string, string> = {}
  return {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => { store[k] = v },
    removeItem: (k: string) => { delete store[k] },
    clear: () => { store = {} },
  }
}

describe('platformDesktopTier utilities', () => {
  beforeEach(() => {
    vi.stubGlobal('localStorage', makeLocalStorageMock())
    vi.stubGlobal('window', { dispatchEvent: vi.fn() })
    vi.stubGlobal('CustomEvent', class extends Event { constructor(type: string, init?: CustomEventInit) { super(type, init) } })
  })

  it('loadPlatformDesktopTier returns normal when storage is empty', () => {
    expect(loadPlatformDesktopTier()).toBe('normal')
  })

  it('savePlatformDesktopTier writes to localStorage', () => {
    savePlatformDesktopTier('power')
    expect(localStorage.getItem(PLATFORM_DESKTOP_TIER_KEY)).toBe('power')
  })

  it('loadPlatformDesktopTier reads back saved tier', () => {
    savePlatformDesktopTier('advanced')
    expect(loadPlatformDesktopTier()).toBe('advanced')
  })

  it('loadPlatformDesktopTier ignores invalid values and falls back to normal', () => {
    localStorage.setItem(PLATFORM_DESKTOP_TIER_KEY, 'ultra')
    expect(loadPlatformDesktopTier()).toBe('normal')
  })

  it('savePlatformDesktopTier round-trips all valid tiers', () => {
    for (const tier of ['normal', 'power', 'advanced'] as const) {
      savePlatformDesktopTier(tier)
      expect(loadPlatformDesktopTier()).toBe(tier)
    }
  })
})
