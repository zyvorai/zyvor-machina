// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { buildConsoleAccessPolicy, permissionsFromPlan } from './consolePermissions'
import type { ConsoleHubPlan } from '../api/platform'

const basePlan: ConsoleHubPlan = {
  vm_id: 'v1',
  vm_name: 'vm-1',
  recommended: 'novnc',
  native: { console_type: 'vnc', ws_path: '/ws', available: true },
  os_hint: 'linux',
  protocols: ['novnc'],
  webrtc_spice_available: false,
  session_recording_enabled: true,
  permissions: {
    role: 'readonly',
    read_only: true,
    can_power: false,
    can_snapshot: false,
    can_send_keys: false,
  },
}

describe('consolePermissions', () => {
  it('maps plan permissions for readonly role', () => {
    expect(permissionsFromPlan(basePlan).readOnly).toBe(true)
    expect(permissionsFromPlan(basePlan).canPower).toBe(false)
  })

  it('builds recording watermark for active policy', () => {
    const policy = buildConsoleAccessPolicy({ plan: basePlan, username: 'admin' })
    expect(policy.recordingActive).toBe(true)
    expect(policy.watermarkLabel).toContain('Recorded')
  })

  it('forces read-only for valid spectator tokens', () => {
    const policy = buildConsoleAccessPolicy({
      plan: basePlan,
      spectatorValid: true,
      spectatorActor: 'support',
    })
    expect(policy.readOnly).toBe(true)
    expect(policy.canPower).toBe(false)
    expect(policy.watermarkLabel).toContain('Read-only')
  })
})
