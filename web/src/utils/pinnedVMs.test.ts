// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getPinnedVMs, togglePin } from './pinnedVMs'

describe('pinnedVMs', () => {
  beforeEach(() => {
    const store = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v) },
      removeItem: (k: string) => { store.delete(k) },
    })
  })

  it('toggles a VM name in and out of the pinned list', () => {
    togglePin('alpha')
    expect(getPinnedVMs()).toEqual(['alpha'])
    togglePin('alpha')
    expect(getPinnedVMs()).toEqual([])
  })

  it('caps unbounded growth — pinning past the limit evicts the oldest pin', () => {
    for (let i = 0; i < 105; i++) togglePin(`vm-${i}`)
    const pinned = getPinnedVMs()
    expect(pinned.length).toBeLessThanOrEqual(100)
    // Oldest pins (vm-0..vm-4) were evicted to make room for the newest.
    expect(pinned).not.toContain('vm-0')
    expect(pinned).toContain('vm-104')
  })
})
