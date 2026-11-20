// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { bySeverityDesc, visibleItems } from './topN'

const row = (id: string, severity?: string) => ({ id, severity })

describe('visibleItems', () => {
  const items = Array.from({ length: 8 }, (_, i) => row(String(i)))
  it('shows the first N and counts the rest', () => {
    const { shown, hidden } = visibleItems(items, false, 5)
    expect(shown.map((x) => x.id)).toEqual(['0', '1', '2', '3', '4'])
    expect(hidden).toBe(3)
  })
  it('shows everything when expanded or when at or under the limit', () => {
    expect(visibleItems(items, true, 5)).toMatchObject({ hidden: 0 })
    expect(visibleItems(items, true, 5).shown).toHaveLength(8)
    expect(visibleItems(items.slice(0, 5), false, 5)).toMatchObject({ hidden: 0 })
  })
  it('ranks before truncating so the worst rows are never the hidden ones', () => {
    const rows = [row('a', 'low'), row('b', 'critical'), row('c', 'warning'), row('d', 'critical'), row('e', 'info'), row('f', 'high')]
    const { shown } = visibleItems(rows, false, 3, bySeverityDesc)
    expect(shown.map((x) => x.id)).toEqual(['b', 'd', 'f'])
  })
  it('is stable for ties and does not mutate its input', () => {
    const rows = [row('x', 'high'), row('y', 'high'), row('z', 'high')]
    const copy = [...rows]
    expect(visibleItems(rows, true, 1, bySeverityDesc).shown.map((r) => r.id)).toEqual(['x', 'y', 'z'])
    expect(rows).toEqual(copy)
  })
})

describe('bySeverityDesc', () => {
  it('sorts unknown and missing severities last', () => {
    const rows = [row('u', 'weird'), row('n'), row('c', 'CRITICAL'), row('i', 'info')]
    expect([...rows].sort(bySeverityDesc).map((r) => r.id)).toEqual(['c', 'i', 'u', 'n'])
  })
})
