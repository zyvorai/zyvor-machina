// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect as baseExpect, type Page, type Route } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const expect = baseExpect.configure({ timeout: 15_000 })

const json = (route: Route, body: unknown, status = 200) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

const base = {
  host_id: 'h1',
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
}
const web = { ...base, id: 'vm-web', name: 'web-1', desired_state: 'running', observed_state: 'running', guest_ip: '10.0.0.5' }
const batch = {
  ...base,
  id: 'vm-batch',
  name: 'batch-1',
  desired_state: 'sleeping',
  observed_state: 'shutoff',
  lifecycle_phase: 'sleeping',
  guest_ip: '10.0.0.6',
}

async function mockSleep(page: Page) {
  const calls: string[] = []
  const policies: Record<string, number | null> = { 'vm-web': null, 'vm-batch': 30 }
  const view = (id: string) => {
    const vm = id === 'vm-web' ? web : batch
    const own = policies[id]
    return {
      vm_id: id,
      desired_state: vm.desired_state,
      observed_state: vm.observed_state,
      sleep_after_minutes: own,
      project: 'dev',
      project_default: 60,
      effective_minutes: own ?? 60,
      last_active_at: '2026-10-05T08:00:00Z',
      idle_minutes: 12.5,
      slept_at: vm.desired_state === 'sleeping' ? '2026-10-05T07:00:00Z' : null,
      wakeable: true,
      events: vm.desired_state === 'sleeping' ? [{ kind: 'sleep', reason: 'idle 31 min', at: '2026-10-05 07:00:00' }] : [],
    }
  }
  await page.route(/\/api\/v1\/vms(\?[^/]*)?$/, (route) => json(route, [web, batch]))
  await page.route(/\/api\/v1\/vms\/vm-(web|batch)$/, (route) =>
    json(route, route.request().url().endsWith('vm-web') ? web : batch),
  )
  await page.route(/\/api\/v1\/vms\/vm-[a-z]+\/(disks|nics)$/, (route) => json(route, []))
  await page.route(/\/api\/v1\/vms\/vm-[a-z]+\/(sleep|wake)$/, (route) => {
    const parts = new URL(route.request().url()).pathname.split('/')
    calls.push(`${parts.at(-1)} ${parts.at(-2)}`)
    return json(route, { task_id: 't-1', status: 'pending', operation: `vm.${parts.at(-1)}` })
  })
  await page.route(/\/api\/v1\/vms\/vm-[a-z]+\/sleep-policy$/, (route) => {
    const id = new URL(route.request().url()).pathname.split('/').at(-2) as string
    if (route.request().method() === 'PUT') {
      const b = route.request().postDataJSON() as { sleep_after_minutes: number | null }
      policies[id] = b.sleep_after_minutes
      calls.push(`policy ${id} ${b.sleep_after_minutes}`)
    }
    return json(route, view(id))
  })
  return calls
}

test('Fleet Cloud instances: sleeping filter, sleep and wake actions', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const calls = await mockSleep(page)
  await page.goto('/fleet-cloud/instances')
  await expect(page.getByRole('link', { name: 'web-1' })).toBeVisible()
  await expect(page.getByText('SLEEPING', { exact: true }).last()).toBeVisible()

  await page.getByRole('button', { name: 'SLEEPING', exact: true }).click()
  await expect(page.getByRole('link', { name: 'batch-1' })).toBeVisible()
  await expect(page.getByRole('link', { name: 'web-1' })).toHaveCount(0)

  await page.getByRole('button', { name: 'Wake batch-1' }).click()
  await expect.poll(() => calls).toContain('wake vm-batch')

  await page.getByRole('button', { name: 'All', exact: true }).click()
  await page.getByRole('button', { name: 'Sleep web-1' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Sleep' }).click()
  await expect.poll(() => calls).toContain('sleep vm-web')
})

test('Fleet Cloud instance detail: scale to zero card', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  const calls = await mockSleep(page)
  await page.goto('/fleet-cloud/instances/vm-web')
  const card = page.getByRole('region', { name: 'Scale to zero' })
  await expect(card).toBeVisible()
  await expect(card.getByText('Sleeps after 60 min without CPU or network activity. Idle for 13 min.')).toBeVisible()
  await expect(card.getByLabel('Auto-sleep')).toHaveValue('inherit')
  await card.getByLabel('Auto-sleep').selectOption('30')
  await expect.poll(() => calls).toContain('policy vm-web 30')
  await card.getByLabel('Auto-sleep').selectOption('0')
  await expect.poll(() => calls).toContain('policy vm-web 0')
  await expect(card.getByText('Auto-sleep is off.')).toBeVisible()
  await card.getByRole('button', { name: 'Sleep now' }).click()
  await expect.poll(() => calls).toContain('sleep vm-web')

  await page.goto('/fleet-cloud/instances/vm-batch')
  const asleep = page.getByRole('region', { name: 'Scale to zero' })
  await expect(asleep.getByText(/Sleeping — memory is on disk/)).toBeVisible()
  await expect(asleep.getByText(/slept \(idle 31 min\)/)).toBeVisible()
  await asleep.getByRole('button', { name: 'Wake' }).click()
  await expect.poll(() => calls).toContain('wake vm-batch')
})
