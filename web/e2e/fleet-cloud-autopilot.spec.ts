// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const expect = baseExpect.configure({ timeout: 15_000 })

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

type Call = { path: string; method: string; body: unknown }

function recorder(calls: Call[]) {
  return (route: Route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname.replace(/^.*\/api\/v1/, '')
    const body = req.postData() ? req.postDataJSON() : null
    if (req.method() !== 'GET') calls.push({ path, method: req.method(), body })
    return path
  }
}

const rec = {
  vm_id: 'vm-1', name: 'api-1', project: 'apps', vcpus: 8, memory_mib: 16384, suggested_vcpus: 2, suggested_memory_mib: 4352,
  cpu_p95: 15, mem_p95: 0.2, hours: 120, monthly_delta_usd: -96.4, reasons: ['CPU p95 15% of 8 vCPUs', 'memory p95 20%'], pending_action: null,
}

async function mockAutopilot(page: Page, calls: Call[], opts: { empty?: boolean } = {}) {
  const record = recorder(calls)
  await page.route(/\/api\/v1\/rightsizing(\/.*)?$/, (route) => {
    const path = record(route)
    if (path === '/rightsizing/propose') return json(route, { id: 'act-r', label: 'Resize api-1', status: 'pending' })
    return json(route, opts.empty ? { recommendations: [], monthly_delta_usd: 0, min_hours: 72 } : { recommendations: [rec], monthly_delta_usd: -96.4, min_hours: 72 })
  })
  await page.route(/\/api\/v1\/drs\/consolidation(\/.*)?$/, (route) => {
    const path = record(route)
    if (path === '/drs/consolidation/propose') return json(route, { id: 'act-c', label: 'Consolidate', status: 'pending' })
    return json(route, opts.empty
      ? { moves: [], emptied: [], kept: [] }
      : {
          moves: [{ vm_id: 'vm-9', vm: 'batch-1', from: 'h2', to: 'h1', to_name: 'kvm-a' }],
          emptied: [{ id: 'h2', name: 'kvm-b', memory_total_mib: 65536, memory_used_mib: 4096 }],
          kept: [['kvm-c', 'runs a VM pinned to its host']],
        })
  })
  await page.route(/\/api\/v1\/ai\/actions\/[^/]+\/execute$/, (route) => { record(route); return json(route, { message: 'ok' }) })
}

test.describe('Fleet Cloud autopilot', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page)
  })

  test('applies a rightsizing suggestion through an approval action', async ({ page }) => {
    const calls: Call[] = []
    await mockAutopilot(page, calls)
    await page.goto('/fleet-cloud/autopilot')
    await expect(page.getByText('8 → 2 vCPUs · 16 GiB → 4.3 GiB · −$96.40/month')).toBeVisible()
    await page.getByRole('button', { name: 'Apply resize of api-1' }).click()
    await expect.poll(() => calls.map((c) => c.path)).toEqual(['/rightsizing/propose', '/ai/actions/act-r/execute'])
    expect(calls[0].body).toEqual({ vm_id: 'vm-1' })
  })

  test('consolidation lists the moves and the host that can power down', async ({ page }) => {
    const calls: Call[] = []
    await mockAutopilot(page, calls)
    await page.goto('/fleet-cloud/autopilot')
    await expect(page.getByText('1 live migration empties kvm-b, which can then be powered down.')).toBeVisible()
    await expect(page.getByText('batch-1 → kvm-a')).toBeVisible()
    await expect(page.getByText('kvm-c: runs a VM pinned to its host')).toBeVisible()
    await page.getByRole('button', { name: 'Request approval' }).last().click()
    await expect.poll(() => calls.map((c) => c.path)).toEqual(['/drs/consolidation/propose'])
  })

  test('says why nothing is suggested yet', async ({ page }) => {
    await mockAutopilot(page, [], { empty: true })
    await page.goto('/fleet-cloud/autopilot')
    await expect(page.getByText('Nothing to change. Instances need at least 72 hours of history before they get a suggestion.')).toBeVisible()
    await expect(page.getByText('Hosts are already packed well. Nothing to move.')).toBeVisible()
  })

  test('a group saves predictive autoscaling with sleep and load-balancer drain', async ({ page }) => {
    const calls: Call[] = []
    const record = recorder(calls)
    const policy = { min: 1, max: 4, desired: 2, target_cpu: null, cooldown_secs: 300 }
    await page.route('**/api/v1/project-registry', (r) => r.fulfill({ json: [{ id: 'p1', name: 'apps', enabled: true }] }))
    await page.route('**/api/v1/hosts', (r) => r.request().method() === 'GET' ? r.fulfill({ json: [{ id: 'h1', hostname: 'kvm-a', state: 'online' }] }) : r.fallback())
    await page.route('**/api/v1/load-balancers', (r) => json(r, [{ id: 'lb1', project_id: 'p1', name: 'front', protocol: 'tcp', host_id: 'h1', listener_port: 80, status: 'active', status_message: '', created_at: '' }]))
    await page.route('**/api/v1/cloud/projects/p1/vpcs', (r) => json(r, []))
    await page.route('**/api/v1/cloud/projects/p1/launch-templates', (r) => json(r, []))
    await page.route('**/api/v1/cloud/projects/p1/instance-groups', (r) =>
      json(r, [{ id: 'g1', project_id: 'p1', template_id: 't1', subnet_id: 's1', name: 'web', policy_json: JSON.stringify(policy), paused: false, last_scaled_at: '', last_error: '' }]))
    await page.route('**/api/v1/cloud/instance-groups/g1', (route) => {
      record(route)
      if (route.request().method() === 'PATCH') return json(route, { ok: true })
      return json(route, {
        group: {}, scale_in: 'stop-and-retain',
        members: [
          { slot: 0, vm_id: 'a', name: 'web-0', observed_state: 'running', desired_state: 'running', draining_since: null },
          { slot: 1, vm_id: 'b', name: 'web-1', observed_state: 'running', desired_state: 'running', draining_since: '2026-10-05T09:00:00Z' },
        ],
      })
    })
    const now = Math.floor(Date.now() / 3_600_000) * 3600
    await page.route('**/api/v1/cloud/instance-groups/g1/forecast', (r) => json(r, {
      group_id: 'g1', predictive: false, target_cpu: null, hours_of_history: 400,
      history: [{ hour: now - 3600, demand: 90 }],
      forecast: [{ hour: now, demand: 240, basis: 'weekly', needed: 4 }],
      next_hour_peak: { hour: now, demand: 240, basis: 'weekly', needed: 4 },
    }))
    await page.goto('/fleet-cloud/vpcs')
    await page.getByText('Autoscale web').click()
    await expect(page.getByText('draining from the load balancer')).toBeVisible()
    await expect(page.getByText('Next hour: about 240% CPU across the group, 4 instances at the target (weekly pattern).')).toBeVisible()
    await expect(page.getByLabel('Predictive scaling')).toBeDisabled()
    await page.getByLabel('Target CPU percent').fill('60')
    await page.getByLabel('Predictive scaling').check()
    await page.getByLabel('Scale-in mode').selectOption('sleep')
    await page.getByLabel('Load balancer').selectOption('lb1')
    await page.getByLabel('Member port').fill('8080')
    await page.getByLabel('Drain seconds').fill('20')
    await page.getByRole('button', { name: 'Save autoscaling' }).click()
    await expect.poll(() => calls.find((c) => c.method === 'PATCH')?.body).toEqual({
      policy: { ...policy, target_cpu: 60, predictive: true, scale_in: 'sleep', load_balancer: { id: 'lb1', port: 8080 }, drain_secs: 20 },
      paused: false,
    })
  })
})
