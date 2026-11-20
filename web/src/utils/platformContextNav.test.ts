// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { contextNavForPath, isContextNavActive, splitContextNavItems, settingsItemsForTier, type ContextNavItem } from './platformContextNav'

const SAMPLE: ContextNavItem[] = [
  { to: '/platform/zeus/security', label: 'Security Center' },
  { to: '/platform/zeus/security/firewall', label: 'Firewall' },
  { to: '/platform/zeus/security/ports', label: 'Open Ports' },
  { to: '/platform/zeus/security/services', label: 'Allowed Apps' },
  { to: '/platform/zeus/security/activity', label: 'Activity' },
  { to: '/platform/zeus/security/compliance', label: 'Compliance' },
  { to: '/platform/zeus/security/k8s', label: 'Kubernetes' },
  { to: '/platform/zeus/security/policies', label: 'Policy Studio' },
]

describe('splitContextNavItems', () => {
  it('returns all items when under the cap', () => {
    const items = SAMPLE.slice(0, 4)
    const result = splitContextNavItems(items, '/platform/zeus/security/firewall', '')
    expect(result.visible).toHaveLength(4)
    expect(result.overflow).toHaveLength(0)
  })

  it('splits overflow when over the cap and active item is in primary range', () => {
    const result = splitContextNavItems(SAMPLE, '/platform/zeus/security/firewall', '', 6)
    expect(result.visible).toHaveLength(6)
    expect(result.overflow.length).toBeGreaterThan(0)
    expect(result.visible.some((item) => item.to === '/platform/zeus/security/firewall')).toBe(true)
  })

  it('keeps active item visible when it would otherwise overflow', () => {
    const result = splitContextNavItems(SAMPLE, '/platform/zeus/security/policies', '', 6)
    expect(result.visible.some((item) => item.to === '/platform/zeus/security/policies')).toBe(true)
    expect(result.overflow.some((item) => item.to === '/platform/zeus/security/policies')).toBe(false)
  })
})

describe('isContextNavActive', () => {
  it('matches settings sections via query params', () => {
    const item = { to: '/platform/settings?section=network', label: 'Network' }
    expect(isContextNavActive('/platform/settings', '?section=network', item)).toBe(true)
    expect(isContextNavActive('/platform/settings', '?section=general', item)).toBe(false)
  })

  it('matches hub roots exactly without catching child routes', () => {
    const hub = { to: '/platform/operations', label: 'Overview' }
    expect(isContextNavActive('/platform/operations', '', hub)).toBe(true)
    expect(isContextNavActive('/platform/operations/tasks', '', hub)).toBe(false)
  })
})

describe('settingsItemsForTier', () => {
  it('includes power sections only for Power user and above', () => {
    const normal = settingsItemsForTier('normal').map((item) => item.label)
    const power = settingsItemsForTier('power').map((item) => item.label)
    expect(normal).not.toContain('Policy')
    expect(power).toContain('Policy')
    expect(power.length).toBeGreaterThan(normal.length)
  })

  it('overflows advanced settings into More menu groups', () => {
    const items = settingsItemsForTier('advanced')
    const result = splitContextNavItems(items, '/platform/settings', '?section=policy', 6)
    expect(result.visible.some((item) => item.label === 'Policy')).toBe(true)
    expect(result.overflow.length).toBeGreaterThan(0)
  })

  it('includes observability in operations context on power tier', () => {
    const ctx = contextNavForPath('/platform/tasks', 'power')
    expect(ctx?.items.some((item) => item.label === 'Observability')).toBe(true)
  })

  it('shows settings context on full policy workspace', () => {
    const ctx = contextNavForPath('/platform/policy', 'power')
    expect(ctx?.appLabel).toBe('Settings')
    expect(ctx?.items.some((item) => item.label === 'Policy')).toBe(true)
    const policy = { to: '/platform/settings?section=policy', label: 'Policy' }
    expect(isContextNavActive('/platform/policy', '', policy)).toBe(true)
  })
})
