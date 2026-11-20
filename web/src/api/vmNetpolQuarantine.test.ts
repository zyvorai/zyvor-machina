// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  THREAT_FEED_NAME,
  describeAllow,
  describeJit,
  draftVmNetpol,
  formatRemaining,
  groupQuarantines,
  splitDomains,
  threatDomainCount,
  type VmQuarantine,
} from './vmNetpol'

const q = (over: Partial<VmQuarantine>): VmQuarantine => ({
  vm: 'web-1',
  since: '2026-10-04T08:00:00Z',
  until: '2026-10-04T09:00:00Z',
  remaining_secs: 100,
  allow: [],
  taps: [],
  ...over,
})

describe('quarantine helpers', () => {
  it('folds a fleet quarantine held by every host into one row per VM', () => {
    const rows = groupQuarantines([
      q({ hostname: 'hv1', taps: [] }),
      q({ hostname: 'hv2', taps: ['vnet3'], remaining_secs: 120, until: '2026-10-04T09:00:02Z' }),
      q({ vm: 'db-1', hostname: 'hv1', taps: ['vnet1'] }),
    ])
    expect(rows.map((r) => r.vm)).toEqual(['db-1', 'web-1'])
    expect(rows[1].hosts).toEqual(['hv2'])
    expect(rows[1].taps).toEqual(['vnet3'])
    expect(rows[1].remaining_secs).toBe(120)
    expect(rows[1].until).toBe('2026-10-04T09:00:02Z')
  })

  it('formats remaining time and exceptions', () => {
    expect(formatRemaining(0)).toBe('expiring')
    expect(formatRemaining(42)).toBe('42s left')
    expect(formatRemaining(600)).toBe('10m left')
    expect(formatRemaining(3 * 3600 + 120)).toBe('3h 2m left')
    expect(describeAllow({ direction: 'ingress', peer: 'host', proto: 'tcp', port: 22 })).toBe('ingress host tcp/22')
    expect(describeAllow({ direction: 'egress', peer: 'world' })).toBe('egress world any')
  })

  it('describes temporary access', () => {
    expect(describeJit({ from: 'web-1', to: 'db-1', port: 5432, protocol: 'TCP' })).toBe('web-1 → db-1:5432/tcp')
    expect(describeJit({ from: 'host', to: 'db-1', port: 0, protocol: 'ANY' })).toBe('host → db-1 (every port)')
  })

  it('reads threat feed input and counts', () => {
    expect(splitDomains('evil.example\n c2.example, ads.example  ')).toEqual(['evil.example', 'c2.example', 'ads.example'])
    expect(threatDomainCount({ name: 'a', source: '', block: false, domains: 3 })).toBe(3)
    expect(threatDomainCount({ name: 'a', source: '', block: true, domain_count: 7 })).toBe(7)
    expect(THREAT_FEED_NAME.test('url-haus_1.0')).toBe(true)
    expect(THREAT_FEED_NAME.test('bad name')).toBe(false)
  })
})

describe('plain-English drafts', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('sends rules_only to the controller only and surfaces why nothing was drafted', async () => {
    const calls: Array<{ url: string; body: unknown }> = []
    vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
      calls.push({ url, body: JSON.parse(String(init.body)) })
      if (calls.length === 3) {
        return new Response(JSON.stringify({ error: 'could not draft a policy from that description: make it nice — not understood' }), {
          status: 422,
          headers: { 'content-type': 'application/json' },
        })
      }
      return new Response(JSON.stringify({ yaml: 'a', source: 'rules', notes: [], unparsed: [] }), {
        headers: { 'content-type': 'application/json' },
      })
    })
    await draftVmNetpol('host', 'isolate db-1')
    await draftVmNetpol('fleet', 'isolate db-1', true)
    expect(calls[0]).toEqual({ url: '/api/v1/vm-network-policies/draft', body: { prompt: 'isolate db-1' } })
    expect(calls[1].url).toMatch(/platform\/controller\/api\/v1\/vm-network-policies\/draft$/)
    expect(calls[1].body).toEqual({ prompt: 'isolate db-1', rules_only: true })
    await expect(draftVmNetpol('host', 'make it nice')).rejects.toThrow(/not understood/)
  })
})
