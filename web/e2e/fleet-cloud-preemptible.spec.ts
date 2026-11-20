// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const expect = baseExpect.configure({ timeout: 15_000 })

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

type Call = { path: string; method: string; body: unknown }

const overview = {
  settings: { enabled: true, reserve_pct: 10 },
  resume_margin_pct: 5,
  hosts: [{ id: 'h1', name: 'kvm-a', total_mib: 32768, used_mib: 31000, free_mib: 1768, reserve_mib: 3276, preemptible_running_mib: 2048, fresh: true }],
  vms: [
    { id: 'v1', name: 'batch-1', host_id: 'h1', host: 'kvm-a', project: 'jobs', memory_mib: 2048, priority: 0, desired_state: 'sleeping', observed_state: 'shutoff', preempted_at: '2026-10-05T10:00:00Z' },
    { id: 'v2', name: 'batch-2', host_id: 'h1', host: 'kvm-a', project: 'jobs', memory_mib: 2048, priority: 20, desired_state: 'running', observed_state: 'running', preempted_at: null },
  ],
  events: [{ vm: 'batch-1', kind: 'sleep', reason: 'preempted: kvm-a below 10% free memory', at: '2026-10-05T10:00:00Z' }],
}

async function mock(page: Page, calls: Call[]) {
  await page.route('**/api/v1/vms', (r) => r.request().method() === 'GET'
    ? json(r, [{ id: 'v1', name: 'batch-1' }, { id: 'v2', name: 'batch-2' }, { id: 'v3', name: 'ci-runner' }])
    : r.fallback())
  await page.route(/\/api\/v1\/(preemption(\/settings)?|vms\/[^/]+\/preemptible)$/, (route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname.replace(/^.*\/api\/v1/, '')
    if (req.method() !== 'GET') {
      const body = req.postDataJSON()
      calls.push({ path, method: req.method(), body })
      return json(route, body)
    }
    return json(route, overview)
  })
}

test.describe('Fleet Cloud preemptible instances', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page)
  })

  test('shows hosts under pressure and preempted instances', async ({ page }) => {
    await mock(page, [])
    await page.goto('/fleet-cloud/preemptible')
    await expect(page.getByText('kvm-a · below its reserve')).toBeVisible()
    await expect(page.getByText('1.7 GiB free of 32 GiB · reserve 3.2 GiB · 2.0 GiB in running preemptible instances')).toBeVisible()
    await expect(page.getByText('Preempted since 2026-10-05 10:00 · 2.0 GiB on kvm-a')).toBeVisible()
    await expect(page.getByText('batch-1: preempted: kvm-a below 10% free memory')).toBeVisible()
  })

  test('saves the reserve, priorities and the flag', async ({ page }) => {
    const calls: Call[] = []
    await mock(page, calls)
    await page.goto('/fleet-cloud/preemptible')
    await page.getByLabel('Free memory reserve').fill('15')
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    await page.getByLabel('Priority of batch-2', { exact: true }).fill('40')
    await page.getByRole('button', { name: 'Save priority of batch-2' }).click()
    await page.getByLabel('Instance to make preemptible').selectOption('v3')
    await page.getByLabel('New priority').fill('5')
    await page.getByRole('button', { name: 'Make preemptible' }).click()
    await page.getByRole('button', { name: 'Stop preempting batch-1' }).click()
    await expect.poll(() => calls).toEqual([
      { path: '/preemption/settings', method: 'PUT', body: { enabled: true, reserve_pct: 15 } },
      { path: '/vms/v2/preemptible', method: 'PUT', body: { preemptible: true, priority: 40 } },
      { path: '/vms/v3/preemptible', method: 'PUT', body: { preemptible: true, priority: 5 } },
      { path: '/vms/v1/preemptible', method: 'PUT', body: { preemptible: false, priority: 0 } },
    ])
  })

  test('create sends the preemptible flag and priority', async ({ page }) => {
    let sent: Record<string, unknown> | null = null
    await page.route('**/api/v1/flavors', (r) => json(r, [{ id: 'f1', name: 'small', vcpus: 1, memory_mib: 1024, disk_gib: 10 }]))
    await page.route('**/api/v1/templates', (r) => json(r, [{ id: 't1', name: 'debian-12', os_family: 'linux', category: 'os' }]))
    await page.route('**/api/v1/networks', (r) => json(r, [{ id: 'n1', name: 'default', backend: 'nat' }]))
    await page.route('**/api/v1/vms/from-template', (r) => { sent = r.request().postDataJSON(); return json(r, { task_id: 't', status: 'pending', operation: 'vm.create' }) })
    await page.goto('/fleet-cloud/create')
    await page.getByLabel('Instance name').fill('batch-9')
    await page.getByText('debian-12').click()
    await page.getByRole('checkbox', { name: 'Give way when capacity is short' }).check()
    await page.getByLabel('Preemption priority').fill('30')
    await page.getByRole('button', { name: 'Create instance' }).click()
    await expect.poll(() => sent).toMatchObject({ name: 'batch-9', preemptible: true, preempt_priority: 30 })
  })
})
