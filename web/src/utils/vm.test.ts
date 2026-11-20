// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { formatBytes, formatThroughput } from './vm'

describe('formatBytes', () => {
  it('labels binary divisors with binary (GiB/MiB/KiB) units', () => {
    // Regression: previously divided by 1024 but labeled GB/MB/KB, so a value
    // read "465.7 GB" for what is actually 500 GiB worth of bytes.
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(512)).toBe('512 B')
    expect(formatBytes(1024)).toBe('1.0 KiB')
    expect(formatBytes(1024 * 1024)).toBe('1.0 MiB')
    expect(formatBytes(1024 * 1024 * 1024)).toBe('1.0 GiB')
    expect(formatBytes(1.5 * 1024 * 1024 * 1024)).toBe('1.5 GiB')
  })

  it('returns an em dash for non-finite input instead of "NaN GB"', () => {
    expect(formatBytes(Number.NaN)).toBe('—')
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe('—')
  })

  it('formatThroughput appends /s', () => {
    expect(formatThroughput(1024)).toBe('1.0 KiB/s')
  })
})
