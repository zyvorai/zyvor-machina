// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import type { VmFlowEdge } from '../api/vmNetpol'
import { buildServiceMap, edgeTone } from './serviceMap'

const e = (src: string, dst: string, direction: string, port: number, extra: Partial<VmFlowEdge> = {}): VmFlowEdge => ({
  src,
  dst,
  src_vm: src.includes('.') ? null : src,
  dst_vm: dst.includes('.') ? null : dst,
  direction,
  proto: 'TCP',
  port,
  verdict: 'FORWARDED',
  count: 10,
  bytes: 600,
  first_seen: '2026-10-01T00:00:00Z',
  last_seen: '2026-10-02T00:00:00Z',
  ...extra,
})

describe('buildServiceMap', () => {
  it('merges both taps of a VM pair and lays clients left of servers', () => {
    const g = buildServiceMap([
      e('web', 'db', 'egress', 5432),
      e('web', 'db', 'ingress', 5432),
      e('db', '10.9.9.9', 'egress', 443, { verdict: 'AUDIT', drop_reason: 'default-deny' }),
      e('db', '10.0.0.1', 'egress', 53, { proto: 'UDP', dst_entity: 'host' }),
    ])
    expect(g.nodes.find((n) => n.id === '10.0.0.1')!.label).toBe('host · 10.0.0.1')
    const link = g.links.find((l) => l.id === 'web→db')!
    expect(link.count).toBe(10)
    expect(link.ports).toEqual(['5432/tcp'])
    const x = (id: string) => g.nodes.find((n) => n.id === id)!.x
    expect(x('web')).toBeLessThan(x('db'))
    expect(x('db')).toBeLessThan(x('10.9.9.9'))
    expect(g.links.find((l) => l.dst === '10.9.9.9')!.tone).toBe('warn')
  })

  it('ranks a client left of its server even when both also talk out', () => {
    const g = buildServiceMap([
      e('client', 'server', 'egress', 80),
      e('client', '10.0.0.1', 'egress', 53, { dst_entity: 'host' }),
      e('server', '10.0.0.1', 'egress', 53, { dst_entity: 'host' }),
      e('10.0.0.1', 'server', 'ingress', 22, { src_entity: 'host' }),
    ])
    const x = (id: string) => g.nodes.find((n) => n.id === id)!.x
    expect(x('client')).toBeLessThan(x('server'))
    expect(x('server')).toBeLessThan(x('10.0.0.1'))
  })

  it('folds many external addresses into other and merges L7 stats', () => {
    const edges = Array.from({ length: 15 }, (_, i) => e('web', `1.1.1.${i}`, 'egress', 443, { count: i + 1 }))
    edges.push(
      e('web', 'api', 'egress', 80, {
        l7: [{ kind: 'http', request: 'GET api/x', count: 2, denied: 1, status: { '2xx': 1 }, latency_n: 1, latency_ms_total: 5, latency_ms_max: 5 }],
      }),
      e('web', 'api', 'egress', 80, {
        verdict: 'DROPPED',
        l7: [{ kind: 'http', request: 'GET api/x', count: 3, denied: 0, status: { '2xx': 2, '5xx': 1 }, latency_n: 3, latency_ms_total: 30, latency_ms_max: 20 }],
      }),
    )
    const g = buildServiceMap(edges)
    expect(g.collapsed).toBe(3)
    expect(g.nodes.some((n) => n.id === 'other' && n.kind === 'world')).toBe(true)
    const api = g.links.find((l) => l.id === 'web→api')!
    expect(api.tone).toBe('error')
    expect(api.l7[0]).toMatchObject({ count: 5, denied: 1, latency_n: 4, latency_ms_max: 20, status: { '2xx': 3, '5xx': 1 } })
    expect(api.edges.map(edgeTone).sort()).toEqual(['error', 'ok'])
  })
})
