// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import type { PlatformVm } from '../../../api/platform'
import { vmFeatures } from './VmFeatureChips'

const vm = (over: Partial<PlatformVm>): PlatformVm => ({
  id: 'v1', name: 'vm', desired_state: 'running', observed_state: 'running', lifecycle_phase: 'ready',
  last_error: '', managed: true, vcpus: 1, memory_mib: 1024, ha_enabled: false, tags: [], inventory_source: 'libvirt',
  ...over,
})

describe('vmFeatures', () => {
  it('words a missing guest agent plainly', () => {
    const chips = vmFeatures(vm({ guest_tools_status: 'not_installed' }), null)
    expect(chips.find((c) => c.key === 'qga')?.label).toBe('Guest agent not installed')
  })
  it('marks a running agent ok and flags discovered VMs', () => {
    const chips = vmFeatures(vm({ guest_tools_status: 'running', managed: false }), null)
    expect(chips.find((c) => c.key === 'qga')?.tone).toBe('ok')
    expect(chips.some((c) => c.key === 'disc')).toBe(true)
  })
})
