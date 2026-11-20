// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { vmHealthScore } from './vmHealthScore'

describe('vmHealthScore', () => {
  it('prefers score_numeric over the label in score', () => {
    expect(vmHealthScore({ score: 'warning', score_numeric: 63 })).toBe(63)
  })
  it('falls back to a numeric score string', () => {
    expect(vmHealthScore({ score: '88' })).toBe(88)
  })
  it('returns null for labels only or missing reports', () => {
    expect(vmHealthScore({ score: 'warning' })).toBeNull()
    expect(vmHealthScore(null)).toBeNull()
  })
  it('clamps to 0-100 and keeps a real zero', () => {
    expect(vmHealthScore({ score_numeric: 0 })).toBe(0)
    expect(vmHealthScore({ score_numeric: 140 })).toBe(100)
  })
})
