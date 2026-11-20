// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  platformDesktopTabGroup,
  upsertPlatformDesktopTab,
} from './platformDesktopTabs'

describe('platformDesktopTabs', () => {
  beforeEach(() => {
    const store = new Map<string, string>()
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => { store.set(k, v) },
      removeItem: (k: string) => { store.delete(k) },
    })
    vi.stubGlobal('window', {
      dispatchEvent: vi.fn(),
    })
  })

  it('groups VM child routes under /platform/vms', () => {
    expect(platformDesktopTabGroup('/platform/vms/dead-beef/console')).toBe('/platform/vms')
    expect(platformDesktopTabGroup('/platform/vms')).toBe('/platform/vms')
  })

  it('stores hub paths when upserting child routes', () => {
    const tabs = upsertPlatformDesktopTab({
      path: '/platform/vms/dead-beef/console',
      label: 'VM Console',
    })
    expect(tabs.some((t) => t.path === '/platform/vms')).toBe(true)
    expect(tabs.some((t) => t.path.includes('dead-beef'))).toBe(false)
  })
})
