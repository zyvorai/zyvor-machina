// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { effectiveFix } from './VmFixItList'

describe('effectiveFix', () => {
  it('routes install_guest_tools to the guided setup', () => {
    expect(effectiveFix({ id: 'guest_agent', severity: 'warning', message: 'm', fix_action: 'install_guest_tools', fix_label: 'Install guest tools' })?.action).toBe('setup_agent')
  })
  it('gives the unmapped guest agent issues a setup action', () => {
    expect(effectiveFix({ id: 'guest_issue', severity: 'warning', message: 'm' })).toEqual({ action: 'setup_agent', label: 'Set up guest agent' })
  })
  it('keeps real actions and returns null when there is nothing to do', () => {
    expect(effectiveFix({ id: 'managed', severity: 'warning', message: 'm', fix_action: 'adopt_vm', fix_label: 'Adopt VM' })).toEqual({ action: 'adopt_vm', label: 'Adopt VM' })
    expect(effectiveFix({ id: 'other', severity: 'info', message: 'm' })).toBeNull()
  })
})
