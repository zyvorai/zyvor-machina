// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { inferConsoleMonitors, monitorScrollTarget } from './consoleMonitors'

describe('consoleMonitors', () => {
  it('infers dual 1920 panels from 3840x1080', () => {
    const m = inferConsoleMonitors(3840, 1080)
    expect(m).toHaveLength(2)
    expect(m[0]).toMatchObject({ x: 0, width: 1920, height: 1080 })
    expect(m[1]).toMatchObject({ x: 1920, width: 1920, height: 1080 })
  })

  it('returns empty for single 1920x1080', () => {
    expect(inferConsoleMonitors(1920, 1080)).toEqual([])
  })

  it('does not split a single 2560x1080 ultrawide into two monitors', () => {
    // Passes the aspect/width gate but rounds to one 1920-wide panel — a
    // forced minimum of 2 previously misclassified this as dual-monitor.
    expect(inferConsoleMonitors(2560, 1080)).toEqual([])
  })

  it('scroll target for active monitor', () => {
    const m = inferConsoleMonitors(3840, 1080)!
    expect(monitorScrollTarget(m, 1)).toEqual({ left: 1920, top: 0 })
    expect(monitorScrollTarget(m, 'all')).toBeNull()
  })
})
