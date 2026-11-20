// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'
import type { PlatformHost } from '../api/platform'
import { fleetAttention, fleetFacts, formatAge, heartbeatAgeSecs, hostAttention, isControllerHost } from './hostAttention'

const NOW = Date.parse('2026-10-07T12:00:00Z')
const host = (o: Partial<PlatformHost> = {}): PlatformHost => ({
  id: 'h1', hostname: 'n1', address: '10.0.0.5', state: 'online', maintenance_mode: false, agent_grpc_addr: '10.0.0.5:50051',
  vm_count: 2, cpu_percent: 40, memory_used_mib: 2048, memory_total_mib: 8192, fenced: false, schedulable: true,
  validation_status: 'passed', last_heartbeat_at: '2026-10-07T11:59:55Z', site: '', rack: '', ...o,
})

describe('hostAttention', () => {
  it('a healthy host needs nothing', () => { expect(hostAttention(host(), NOW)).toEqual([]) })
  it('flags an offline host with a re-check', () => {
    const a = hostAttention(host({ state: 'offline' }), NOW)
    expect(a[0]).toMatchObject({ kind: 'offline', severity: 'error', fix: 'recheck' })
  })
  it('flags failed validation and stale heartbeats on an online host', () => {
    const kinds = hostAttention(host({ validation_status: 'failed', last_heartbeat_at: '2026-10-07T11:50:00Z' }), NOW).map((a) => a.kind)
    expect(kinds).toContain('validation')
    expect(kinds).toContain('heartbeat')
  })
  it('does not call a fresh heartbeat stale', () => {
    expect(hostAttention(host({ last_heartbeat_at: '2026-10-07T11:59:00Z' }), NOW)).toEqual([])
  })
  it('reports fenced, maintenance and unschedulable, errors first', () => {
    const a = hostAttention(host({ fenced: true, maintenance_mode: true, schedulable: false }), NOW)
    expect(a.map((x) => x.kind)).toEqual(['fenced', 'maintenance'])
    expect(hostAttention(host({ schedulable: false }), NOW)[0].kind).toBe('unschedulable')
  })
  it('fleetAttention sorts errors before info across hosts', () => {
    const items = fleetAttention([host({ id: 'a', maintenance_mode: true }), host({ id: 'b', state: 'offline' })], NOW)
    expect(items.map((i) => i.hostId)).toEqual(['b', 'a'])
  })
})

describe('helpers', () => {
  it('parses SQLite-style and ISO timestamps', () => {
    expect(heartbeatAgeSecs('2026-10-07 11:59:30', NOW)).toBe(30)
    expect(heartbeatAgeSecs('2026-10-07T11:59:30Z', NOW)).toBe(30)
    expect(heartbeatAgeSecs(null, NOW)).toBeNull()
  })
  it('formats ages', () => {
    expect(formatAge(2)).toBe('just now')
    expect(formatAge(30)).toBe('30 s ago')
    expect(formatAge(600)).toBe('10 min ago')
    expect(formatAge(null)).toBe('never')
  })
  it('the controller machine is the one reached over loopback', () => {
    expect(isControllerHost({ agent_grpc_addr: '127.0.0.1:50051', address: '1.2.3.4' })).toBe(true)
    expect(isControllerHost({ agent_grpc_addr: '10.0.0.5:50051', address: '10.0.0.5' })).toBe(false)
  })
  it('fleetFacts averages only reporting hosts and counts hosts needing attention', () => {
    const f = fleetFacts([host({ cpu_percent: 20 }), host({ id: 'b', cpu_percent: 60 }), host({ id: 'c', state: 'offline', cpu_percent: 0 })], NOW)
    expect(f).toMatchObject({ total: 3, online: 2, vms: 6, cpuPercent: 40, memPercent: 25, needAttention: 1 })
  })
})
