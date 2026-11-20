// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createVpc, getCloudPlan, updateInstanceGroup } from './cloud'

beforeEach(() => {
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn(), removeItem: vi.fn() })
  vi.stubGlobal('window', { location: { origin: 'http://localhost:3000', hostname: 'localhost', port: '3000', protocol: 'http:' } })
})
afterEach(() => vi.unstubAllGlobals())
describe('cloud requests', () => {
  it('uses the controller proxy and escapes resource IDs', async () => {
    const calls: Array<{ url: string; init?: RequestInit }> = []
    vi.stubGlobal('fetch', async (url: string, init?: RequestInit) => {
      calls.push({ url, init }); return new Response(JSON.stringify({ id: 'vpc-a', forwarding_active: false }), { headers: { 'content-type': 'application/json' } })
    })
    await createVpc('project/a', { name: 'apps', cidr: '10.20.0.0/16', host_id: 'h1' })
    expect(calls[0].url).toMatch(/platform\/controller\/api\/v1\/cloud\/projects\/project%2Fa\/vpcs$/)
    expect(JSON.parse(String(calls[0].init?.body))).toEqual({ name: 'apps', cidr: '10.20.0.0/16', host_id: 'h1' })
    expect((await getCloudPlan('vpc/a')).forwarding_active).toBe(false)
    expect(calls[1].url).toMatch(/vpcs\/vpc%2Fa\/plan$/)
  })
  it('sends policy and pause together and propagates permission failures', async () => {
    const fetch = vi.fn().mockResolvedValueOnce(new Response('{}', { headers: { 'content-type': 'application/json' } })).mockResolvedValueOnce(new Response('{"error":"project membership required"}', { status: 403, headers: { 'content-type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    const policy = { min: 0, max: 4, desired: 2, target_cpu: 60, cooldown_secs: 300 }
    await updateInstanceGroup('g1', policy, true)
    expect(fetch.mock.calls[0][1].method).toBe('PATCH')
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ policy, paused: true })
    await expect(getCloudPlan('v1')).rejects.toThrow()
  })
})
