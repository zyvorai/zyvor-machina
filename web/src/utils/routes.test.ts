// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { navGroupItems, navGroups, routeLabels } from './routes'

const items = navGroups.flatMap((g) => navGroupItems(g))

describe('navGroups', () => {
  it('has no duplicate destinations within a group', () => {
    for (const g of navGroups) {
      const tos = navGroupItems(g).map((i) => i.to)
      expect(tos.filter((t, i) => tos.indexOf(t) !== i), g.label).toEqual([])
    }
  })

  it('gives every nav path a breadcrumb label', () => {
    const missing = items.map((i) => i.to.split('?')[0]).filter((p) => !(p in routeLabels))
    expect([...new Set(missing)]).toEqual([])
  })

  it('does not reuse an icon for two items in the same section', () => {
    for (const g of navGroups) {
      for (const s of g.sections ?? [{ label: '', items: g.items }]) {
        const icons = s.items.map((i) => (i.icon as { type?: unknown }).type)
        expect(icons.filter((n, i) => icons.indexOf(n) !== i), `${g.label}/${s.label}`).toEqual([])
      }
    }
  })

  it('keeps previously orphaned platform pages reachable', () => {
    const tos = new Set(items.map((i) => i.to))
    for (const p of ['/platform/launchpad', '/platform/support', '/platform/alert-rules', '/platform/scheduled-jobs', '/platform/hosts/finder']) {
      expect(tos.has(p), p).toBe(true)
    }
  })
})
