// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  describeEgress,
  evidenceFilename,
  evidenceQuery,
  getEvidence,
  isProjectPending,
  previewProject,
  resetProject,
  setProject,
  splitPorts,
  type ProjectNet,
} from './vmNetpol'

const net = (over: Partial<ProjectNet>): ProjectNet => ({
  project: 'shop',
  isolation: 'inherit',
  allow_host: true,
  egress_restricted: false,
  egress_allow: [],
  egress_ips: {},
  ...over,
})

describe('project networking helpers', () => {
  it('describes egress allowlists', () => {
    expect(describeEgress(net({}))).toBe('Any destination')
    expect(describeEgress(net({ egress_restricted: true }))).toBe('Only the project, host and DNS')
    expect(
      describeEgress(net({ egress_restricted: true, egress_allow: [{ to: '*.stripe.com', ports: ['443'] }, { to: '10.0.0.0/8' }] })),
    ).toBe('*.stripe.com (443), 10.0.0.0/8')
  })

  it('splits port lists', () => {
    expect(splitPorts('443, 80  53/udp')).toEqual(['443', '80', '53/udp'])
    expect(splitPorts('  ')).toEqual([])
  })

  it('names evidence files by UTC time', () => {
    const at = new Date('2026-10-04T13:05:09Z')
    expect(evidenceFilename('json', at)).toBe('segmentation-evidence-20261004T130509.json')
    expect(evidenceFilename('md', at)).toBe('segmentation-evidence-20261004T130509.md')
  })
})

describe('project networking requests', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('puts project settings to the controller and fetches evidence verbatim', async () => {
    const calls: Array<{ url: string; method?: string; body?: unknown }> = []
    const raw = '{"kind":"machina.io/segmentation-evidence/v1","digest":"ab"}'
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method, body: init?.body ? JSON.parse(String(init.body)) : undefined })
      if (url.includes('/evidence')) return new Response(raw, { headers: { 'content-type': 'application/json' } })
      return new Response(JSON.stringify({ project: net({ isolation: 'isolated' }) }), { headers: { 'content-type': 'application/json' } })
    })
    const { project, ...rest } = net({ isolation: 'isolated' })
    await setProject(project, rest)
    expect(calls[0].url).toMatch(/platform\/controller\/api\/v1\/vm-network-policies\/projects\/shop$/)
    expect(calls[0].method).toBe('PUT')
    expect(calls[0].body).toMatchObject({ isolation: 'isolated', allow_host: true })
    expect(await getEvidence('host', 'json')).toBe(raw)
    expect(calls[1].url).toBe('/api/v1/vm-network-policies/evidence')
    await getEvidence('fleet', 'md')
    expect(calls[2].url).toMatch(/platform\/controller\/api\/v1\/vm-network-policies\/evidence\?format=md$/)
  })

  it('scopes evidence to a project with custom probes, fleet only', async () => {
    const urls: string[] = []
    vi.stubGlobal('fetch', async (url: string) => {
      urls.push(url)
      return new Response('{}', { headers: { 'content-type': 'application/json' } })
    })
    await getEvidence('fleet', 'md', { project: 'shop a', probes: ' tcp/22,udp/53 ' })
    expect(urls[0]).toMatch(/\/evidence\?format=md&project=shop\+a&probes=tcp%2F22%2Cudp%2F53$/)
    await getEvidence('host', 'json', { project: 'shop', probes: 'tcp/80' })
    expect(urls[1]).toBe('/api/v1/vm-network-policies/evidence?probes=tcp%2F80')
    expect(evidenceQuery('json')).toBe('')
    expect(evidenceFilename('json', new Date('2026-10-04T13:05:09Z'), 'shop/a')).toBe('segmentation-evidence-shop_a-20261004T130509.json')
  })

  it('previews, proposes and recognises pending changes', async () => {
    const calls: Array<{ url: string; method?: string }> = []
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      calls.push({ url, method: init?.method })
      const body = url.endsWith('/preview')
        ? { project: 'shop', describe: 'isolated', summary: 'would break 0', replay: { would_break: [], would_allow: [] } }
        : { pending: { id: 'a1' }, preview: 'would break 2 recorded connections' }
      return new Response(JSON.stringify(body), { headers: { 'content-type': 'application/json' } })
    })
    const { project, ...rest } = net({ isolation: 'isolated' })
    const p = await previewProject(project, rest)
    expect(p.summary).toBe('would break 0')
    expect(calls[0]).toMatchObject({ method: 'POST' })
    expect(calls[0].url).toMatch(/projects\/shop\/preview$/)
    const r = await setProject(project, rest, true)
    expect(calls[1].url).toMatch(/projects\/shop\?propose=1$/)
    expect(isProjectPending(r)).toBe(true)
    const d = await resetProject('shop', true)
    expect(calls[2]).toMatchObject({ method: 'DELETE' })
    expect(isProjectPending(d)).toBe(true)
    expect(isProjectPending({ project: net({}) })).toBe(false)
    expect(isProjectPending(null)).toBe(false)
  })
})
