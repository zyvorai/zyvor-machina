// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { clockOf, joinStages, maskToken, terminalLines, type JoinProgress } from './joinProgress'

const ev = (id: string, step: string, level = 'ok', message = step) => ({ id, step, level, message, created_at: '2026-10-07 12:34:56.789' })
const host = (state: string) => ({ id: 'h', hostname: 'n1', address: '10.0.0.5', state, validation_status: 'ok', vm_count: 0, cpu_percent: 1 })
const statuses = (p: JoinProgress | null) => joinStages(p).map((s) => s.status).join(',')

describe('joinStages', () => {
  it('starts at the token and waits for the host', () => {
    expect(statuses({ token_status: 'waiting', events: [ev('1', 'token', 'info')] })).toBe('done,active,pending,pending,pending')
  })
  it('advances as the controller logs steps', () => {
    const p: JoinProgress = { token_status: 'joined', events: [ev('1', 'token', 'info'), ev('2', 'contact', 'info'), ev('3', 'register')] }
    expect(statuses(p)).toBe('done,done,done,active,pending')
  })
  it('is complete only when validated and the host is online', () => {
    const events = [ev('1', 'token'), ev('2', 'contact'), ev('3', 'register'), ev('4', 'validate')]
    expect(statuses({ token_status: 'joined', events, host: host('provisioning') })).toBe('done,done,done,done,active')
    expect(statuses({ token_status: 'joined', events, host: host('online') })).toBe('done,done,done,done,done')
  })
  it('marks validation as failed', () => {
    const p: JoinProgress = { token_status: 'joined', events: [ev('1', 'token'), ev('2', 'contact'), ev('3', 'register'), ev('4', 'validate', 'error')] }
    expect(statuses(p)).toBe('done,done,done,error,pending')
  })
  it('copes with no data yet', () => {
    expect(statuses(null)).toBe('active,pending,pending,pending,pending')
  })
})

describe('terminal formatting', () => {
  it('shows the clock part only', () => {
    expect(clockOf('2026-10-07 12:34:56.789')).toBe('12:34:56.789')
  })
  it('maps levels to symbols and tones', () => {
    const l = terminalLines([ev('1', 'a', 'ok'), ev('2', 'b', 'error'), ev('3', 'c', 'info'), ev('4', 'd', 'weird')])
    expect(l.map((x) => `${x.symbol}${x.tone}`)).toEqual(['✓ok', '✗error', '›info', '›info'])
  })
  it('never shows a whole token', () => {
    const cmd = 'curl x | sudo bash -s -- --token join-1234-abcd'
    expect(maskToken(cmd, 'join-1234-abcd')).toBe('curl x | sudo bash -s -- --token join-1234••••')
  })
})
