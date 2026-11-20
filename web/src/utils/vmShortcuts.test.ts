// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getPinnedVMs } from './pinnedVMs'
import { getRecentVMs } from './recentVMs'
import { purgeVmShortcuts } from './vmShortcuts'

describe('vmShortcuts', () => {
  beforeEach(() => {
    const store = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v) },
      removeItem: (k: string) => { store.delete(k) },
    })
    localStorage.setItem('machina_recent_vms', JSON.stringify(['alpha', 'beta']))
    localStorage.setItem('machina_pinned_vms', JSON.stringify(['beta', 'gamma']))
  })

  it('removes deleted names from recent and pinned lists', () => {
    purgeVmShortcuts(['alpha', 'beta'])
    expect(getRecentVMs()).toEqual([])
    expect(getPinnedVMs()).toEqual(['gamma'])
  })
})
