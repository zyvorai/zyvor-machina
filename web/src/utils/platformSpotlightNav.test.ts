// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { groupSpotlightByZone, spotlightNavForTier, spotlightZoneOrder } from './platformSpotlightNav'

describe('spotlightNavForTier', () => {
  it('groups desktop hubs under Platform hubs zone', () => {
    const entries = spotlightNavForTier('power')
    const hubs = entries.filter((entry) => entry.kind === 'hub')
    expect(hubs.length).toBeGreaterThan(0)
    expect(hubs.every((entry) => entry.zone === 'Platform hubs')).toBe(true)
  })

  it('orders Platform hubs before sidebar zones', () => {
    const order = spotlightZoneOrder()
    expect(order[0]).toBe('Platform hubs')
    const grouped = groupSpotlightByZone(spotlightNavForTier('advanced'))
    expect(grouped[0]?.zone).toBe('Platform hubs')
  })

  it('includes settings panes on power tier', () => {
    const entries = spotlightNavForTier('power')
    const settings = entries.filter((entry) => entry.zone === 'Settings')
    expect(settings.some((entry) => entry.label === 'Policy')).toBe(true)
    expect(settings.every((entry) => entry.path.includes('section='))).toBe(true)
  })

  it('includes operations workspaces on power tier', () => {
    const entries = spotlightNavForTier('power')
    const ops = entries.filter((entry) => entry.zone === 'Operations')
    expect(ops.some((entry) => entry.label === 'Tasks')).toBe(true)
    expect(ops.some((entry) => entry.label === 'Observability')).toBe(true)
  })

  it('includes infrastructure workspaces on power tier', () => {
    const entries = spotlightNavForTier('power')
    const infrastructure = entries.filter((entry) => entry.zone === 'Infrastructure')
    expect(infrastructure.some((entry) => entry.label === 'Networks')).toBe(true)
  })

  it('includes security workspaces on advanced tier', () => {
    const entries = spotlightNavForTier('advanced')
    const security = entries.filter((entry) => entry.zone === 'Security')
    expect(security.some((entry) => entry.label === 'Policy Studio')).toBe(true)
  })

  it('includes zyra workspaces on power tier', () => {
    const entries = spotlightNavForTier('power')
    const zyra = entries.filter((entry) => entry.zone === 'Zyra')
    expect(zyra.some((entry) => entry.label === 'Knowledge')).toBe(true)
    expect(zyra.some((entry) => entry.path.includes('tab=knowledge'))).toBe(true)
  })
})
