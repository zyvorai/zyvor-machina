// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { flattenMacMenuForTier, macMenuSectionsForTier, menubarProductGroupsForTier } from './platformMacMenus'

describe('menubarProductGroupsForTier', () => {
  it('exposes Zeus-style product menus with full catalogs on normal', () => {
    const groups = menubarProductGroupsForTier('normal')
    const labels = groups.map((g) => g.compact)
    expect(labels).toEqual(expect.arrayContaining(['Workloads', 'Infra', 'Ops', 'Secure', 'Admin', 'More']))
    const flat = groups.flatMap((g) => g.sections.flatMap((s) => s.items))
    expect(flat.length).toBeGreaterThan(40)
    expect(flat.some((i) => i.to === '/platform/vms')).toBe(true)
    expect(flat.some((i) => i.to === '/vms')).toBe(true)
    expect(flat.some((i) => i.to === '/platform/zeus/security')).toBe(true)
  })

  it('keeps Advanced-only Policy Studio out of normal/power Secure menus', () => {
    const normal = menubarProductGroupsForTier('normal')
    const power = menubarProductGroupsForTier('power')
    const advanced = menubarProductGroupsForTier('advanced')
    const paths = (tier: typeof normal) =>
      tier.flatMap((g) => g.sections.flatMap((s) => s.items.map((i) => i.to)))
    expect(paths(normal)).not.toContain('/platform/zeus/security/policies')
    expect(paths(power)).not.toContain('/platform/zeus/security/policies')
    expect(paths(advanced)).toContain('/platform/zeus/security/policies')
  })

  it('dedupes paths across product menus', () => {
    const groups = menubarProductGroupsForTier('power')
    const paths = groups.flatMap((g) => g.sections.flatMap((s) => s.items.map((i) => i.to)))
    expect(new Set(paths).size).toBe(paths.length)
  })

  it('puts leftover Host / Fleet destinations under More', () => {
    const groups = menubarProductGroupsForTier('normal')
    const more = groups.find((g) => g.id === 'more')
    expect(more).toBeTruthy()
    const morePaths = more!.sections.flatMap((s) => s.items.map((i) => i.to))
    expect(morePaths).toEqual(expect.arrayContaining(['/platform/zyra', '/fleet-cloud', '/platform']))
  })
})

describe('macMenuSectionsForTier', () => {
  it('mirrors product group compact labels', () => {
    const sections = macMenuSectionsForTier('normal')
    const labels = sections.map((s) => s.label)
    expect(labels).toContain('Workloads')
    expect(labels).toContain('Infra')
  })

  it('flattenMacMenuForTier matches section item count', () => {
    const sections = macMenuSectionsForTier('power')
    const flat = flattenMacMenuForTier('power')
    expect(flat.length).toBe(sections.flatMap((s) => s.items).length)
  })
})
