// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { timeAgo } from './time'

describe('timeAgo', () => {
  const now = Date.now()

  it('buckets seconds / minutes / hours', () => {
    expect(timeAgo(now)).toBe('just now')
    expect(timeAgo(now - 30_000)).toBe('30s ago')
    expect(timeAgo(now - 2 * 60_000)).toBe('2m ago')
    expect(timeAgo(now - 2 * 3600_000)).toBe('2h ago')
  })

  it('has a days bucket (was "50h ago" for 2 days)', () => {
    expect(timeAgo(now - 3 * 86400_000)).toBe('3d ago')
  })

  it('treats a future timestamp (clock skew) as "just now", not a stale value', () => {
    expect(timeAgo(now + 60_000)).toBe('just now')
  })
})
