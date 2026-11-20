// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const expect = baseExpect.configure({ timeout: 15_000 })

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

const vm = {
  id: 'vm-web',
  name: 'web-1',
  host_id: 'h1',
  desired_state: 'running',
  observed_state: 'running',
  lifecycle_phase: 'running',
  last_error: '',
  managed: true,
  uuid: null,
  vcpus: 2,
  memory_mib: 2048,
  ha_enabled: false,
  project: 'dev',
  tags: [],
  inventory_source: 'libvirt',
  guest_tools_status: null,
  guest_ip: '10.0.0.5',
}

const layers = [{ target: 'vda', file: '/var/lib/libvirt/images/web-1.qcow2' }]

async function mockTimeTravel(page: Page) {
  const calls: { path: string; method: string; body: unknown }[] = []
  const state = {
    vm_id: 'vm-web',
    every_minutes: null as number | null,
    keep: 24,
    points: [
      { id: 'rp-1', label: 'rp-20261005T080000-ab12', kind: 'manual', note: 'before upgrade', quiesced: true, created_at: '2026-10-05T08:00:00Z', layers, forks: [] as string[] },
      { id: 'rp-2', label: 'rp-20261005T090000-cd34', kind: 'scheduled', note: null, quiesced: true, created_at: '2026-10-05T09:00:00Z', layers, forks: ['web-1-test'] },
    ],
    forks: [{ vm_id: 'vm-fork', name: 'web-1-test', restore_point_id: 'rp-2', memory: false, isolated: false, created_at: '2026-10-05T09:01:00Z' }],
    fork_of: null,
  }
  await page.route(/\/api\/v1\/vms\/vm-web$/, (route) => json(route, vm))
  await page.route(/\/api\/v1\/vms\/vm-web\/(disks|nics)$/, (route) => json(route, []))
  await page.route(/\/api\/v1\/vms\/vm-web\/sleep-policy$/, (route) => route.fulfill({ status: 404, body: '' }))
  await page.route(/\/api\/v1\/vms\/vm-web\/(restore-points|fork).*$/, (route) => {
    const req = route.request()
    const path = new URL(req.url()).pathname.replace(/^.*\/vms\/vm-web/, '')
    const body = req.postData() ? req.postDataJSON() : null
    if (req.method() !== 'GET') calls.push({ path, method: req.method(), body })
    if (path === '/restore-points/policy') {
      const b = body as { every_minutes: number; keep?: number }
      state.every_minutes = b.every_minutes || null
      if (b.keep) state.keep = b.keep
      return json(route, state)
    }
    if (req.method() === 'GET') return json(route, state)
    return json(route, { task_id: 't-1', status: 'pending', operation: 'vm.x' })
  })
  return calls
}

test('Fleet Cloud instance detail: time travel timeline, rewind and fork', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const calls = await mockTimeTravel(page)
  await page.goto('/fleet-cloud/instances/vm-web')
  const card = page.getByRole('region', { name: 'Time travel' })
  await expect(card).toBeVisible()
  await expect(card.getByText(/2 restore points/)).toBeVisible()
  await expect(card.getByRole('listitem').filter({ hasText: 'web-1-test' })).toBeVisible()

  const slider = card.getByLabel('Point in time')
  await expect(slider).toHaveValue('2')
  await expect(card.getByRole('button', { name: 'Rewind here' })).toBeDisabled()
  await expect(card.getByRole('button', { name: 'Fork now' })).toBeVisible()

  await slider.fill('0')
  await expect(card.getByText(/manual: before upgrade · filesystems quiesced/)).toBeVisible()
  await card.getByRole('button', { name: 'Rewind here' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText(/Everything written after that is discarded and the instance restarts/)).toBeVisible()
  await dialog.getByRole('button', { name: 'Rewind' }).click()
  await expect.poll(() => calls.map((c) => `${c.method} ${c.path}`)).toContain('POST /restore-points/rp-1/rewind')

  await card.getByRole('button', { name: 'Fork here' }).click()
  const form = card.getByRole('form', { name: 'Fork instance' })
  await expect(form.getByLabel('Fork name')).toHaveValue('web-1-fork')
  await expect(form.getByText('Copy memory')).toHaveCount(0)
  await form.getByLabel('Fork name').fill('web-1-debug')
  await form.getByRole('button', { name: 'Fork' }).click()
  await expect.poll(() => calls.find((c) => c.path === '/fork')?.body).toEqual({
    name: 'web-1-debug',
    restore_point_id: 'rp-1',
    memory: false,
    isolate: false,
    reseed: true,
  })

  await slider.fill('2')
  await card.getByRole('button', { name: 'Fork now' }).click()
  const now = card.getByRole('form', { name: 'Fork instance' })
  await now.getByLabel('Fork name').fill('web-1-live')
  await now.getByText(/Copy memory/).click()
  await now.getByRole('button', { name: 'Fork' }).click()
  await expect.poll(() => calls.filter((c) => c.path === '/fork').at(-1)?.body).toEqual({
    name: 'web-1-live',
    memory: true,
    isolate: true,
    reseed: true,
  })

  await card.getByLabel('Restore points').selectOption('60')
  await expect.poll(() => calls.find((c) => c.path === '/restore-points/policy')?.body).toEqual({ every_minutes: 60 })
  await expect(card.getByLabel('Keep')).toHaveValue('24')
  await card.getByRole('button', { name: 'Create restore point' }).click()
  await expect.poll(() => calls.map((c) => `${c.method} ${c.path}`)).toContain('POST /restore-points')
})
