// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { formatVmMemoryGiB, vmLaunchpadGradient, vmSemanticKind, vmStatusBadgeClasses } from './vmVisual'
import { vmStateTone } from './semanticColors'

describe('vmVisual', () => {
  it('normalizes platform and libvirt state strings', () => {
    expect(vmSemanticKind('running')).toBe('running')
    expect(vmSemanticKind('stopped')).toBe('stopped')
    expect(vmSemanticKind('shut off')).toBe('stopped')
    expect(vmSemanticKind('shutoff')).toBe('stopped')
    expect(vmSemanticKind('paused')).toBe('paused')
    expect(vmSemanticKind('missing')).toBe('creating')
    expect(vmSemanticKind('migrating')).toBe('migrating')
    expect(vmSemanticKind('crashed')).toBe('failed')
  })

  it('maps stopped to neutral tone (gray, not red)', () => {
    expect(vmStateTone('stopped')).toBe('neutral')
    expect(vmStateTone('shut off')).toBe('neutral')
    expect(vmStateTone('crashed')).toBe('error')
  })

  it('builds status badge classes', () => {
    expect(vmStatusBadgeClasses('running')).toContain('machina-vm-status--running')
    expect(vmStatusBadgeClasses('paused')).toContain('machina-vm-status--paused')
    expect(vmStatusBadgeClasses('stopped', 'solid')).toContain('machina-vm-status--solid')
  })

  it('picks launchpad gradients per state', () => {
    expect(vmLaunchpadGradient('running')).toContain('emerald')
    expect(vmLaunchpadGradient('paused')).toContain('amber')
    expect(vmLaunchpadGradient('stopped')).toContain('stone')
  })

  it('formats memory safely when mib is missing', () => {
    expect(formatVmMemoryGiB(1024)).toBe('1 GiB')
    expect(formatVmMemoryGiB(undefined)).toBe('—')
    expect(formatVmMemoryGiB(null)).toBe('—')
  })
})
