// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { commandOf, formatLeft, listenerOff, secondsLeft, snippets, tokenState } from './enrollCommand'
import { failedChecks, stageElapsed } from '../../../utils/joinProgress'

describe('enrollCommand', () => {
  it('prefers the pinned join command and falls back to the install command', () => {
    expect(commandOf({ join_command: 'JOIN', install_command: 'INSTALL' })).toBe('JOIN')
    expect(commandOf({ install_command: 'INSTALL' })).toBe('INSTALL')
    expect(listenerOff({})).toBe(true)
    expect(listenerOff({ join_command: 'JOIN' })).toBe(false)
  })
  it('classifies tokens as open, used or expired', () => {
    const now = Date.parse('2026-10-07T12:00:00Z')
    expect(tokenState({ used_at: '2026-10-07 11:00:00', expires_at: '2026-10-08 00:00:00' }, now)).toBe('used')
    expect(tokenState({ used_at: null, expires_at: '2026-10-07 11:59:00' }, now)).toBe('expired')
    expect(tokenState({ used_at: null, expires_at: '2026-10-07T13:00:00Z' }, now)).toBe('open')
  })
  it('counts down and formats the time left', () => {
    const now = Date.parse('2026-10-07T12:00:00Z')
    expect(secondsLeft('2026-10-07 12:30:00', now)).toBe(1800)
    expect(secondsLeft('garbage', now)).toBeNull()
    expect(formatLeft(0)).toBe('expired')
    expect(formatLeft(5400)).toBe('1 h 30 min')
    expect(formatLeft(125)).toBe('2 min 5 s')
  })
  it('puts the same command in every automation snippet that takes one', () => {
    const s = snippets("curl -fsS x | sudo bash -s -- --token T")
    expect(s.cloudInit).toContain('curl -fsS x | sudo bash -s -- --token T')
    expect(s.ansible).toContain('deploy/ansible/site.yml')
    expect(s.terraform).toContain('deploy/terraform/join-nodes')
  })
  it('measures stage timing and picks out failed checks', () => {
    const ev = (id: string, step: string, level: string, message: string, t: string) => ({ id, step, level, message, created_at: `2026-10-07 10:00:${t}` })
    const events = [ev('1', 'token', 'info', 'issued', '00.000'), ev('2', 'contact', 'info', 'c', '03.000'), ev('3', 'check', 'error', 'agent_reachable: no', '09.000')]
    expect(stageElapsed(events)).toMatchObject({ token: 0, contact: 3, check: 9 })
    expect(failedChecks(events)).toEqual(['agent_reachable: no'])
  })
})
