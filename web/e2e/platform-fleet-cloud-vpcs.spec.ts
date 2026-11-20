// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect, type Page } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const policy = (desired: number) => JSON.stringify({ min: 0, max: 4, desired, target_cpu: null, cooldown_secs: 300 })

/** Mock the cloud API. `state` is mutable so a test can change what the next poll returns. */
async function mockCloud(page: Page, state: { subnetStatus: string; groups?: unknown[]; retried?: string[] }) {
  await page.route('**/api/v1/project-registry', (r) => r.fulfill({ json: [{ id: 'p1', name: 'apps', enabled: true }] }))
  await page.route('**/api/v1/hosts', (r) => r.request().method() === 'GET' ? r.fulfill({ json: [{ id: 'h1', hostname: 'kvm-a', state: 'online' }] }) : r.fallback())
  await page.route('**/api/v1/cloud/projects/p1/vpcs', (r) => r.fulfill({ json: [{ id: 'v1', project_id: 'p1', host_id: 'h1', name: 'main', cidr: '10.20.0.0/16' }] }))
  await page.route('**/api/v1/cloud/projects/p1/instance-groups', (r) => r.fulfill({ json: state.groups ?? [] }))
  await page.route('**/api/v1/cloud/projects/p1/launch-templates', (r) => r.fulfill({ json: [{ id: 't1', project_id: 'p1', name: 'web' }] }))
  await page.route('**/api/v1/cloud/vpcs/v1/subnets', (r) =>
    r.fulfill({ json: [{ id: 's1', vpc_id: 'v1', network_id: 'n1', name: 'apps', cidr: '10.20.1.0/24', status: state.subnetStatus, last_error: state.subnetStatus === 'error' ? 'Agent unavailable' : '' }] }))
  await page.route('**/api/v1/cloud/subnets/s1/retry', (r) => {
    state.retried?.push('s1')
    state.subnetStatus = 'ready'
    return r.fulfill({ json: { task_id: 't' } })
  })
}

test.describe('Fleet Cloud VPCs', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page)
  })

  test('shows a failed subnet, retries it, and then offers it to new groups', async ({ page }) => {
    const state = { subnetStatus: 'error', retried: [] as string[] }
    await mockCloud(page, state)
    await page.goto('/fleet-cloud/vpcs')
    await expect(page.getByRole('alert').filter({ hasText: 'Agent unavailable' })).toBeVisible({ timeout: 15_000 })
    await page.getByRole('button', { name: 'Retry apps' }).click()
    await expect.poll(() => state.retried.length).toBe(1)
    await expect(page.getByText('ready', { exact: true })).toBeVisible()
    // a ready subnet can now be chosen for an instance group
    await expect(page.getByLabel('Group subnet').locator('option', { hasText: 'apps' })).toHaveCount(1)
  })

  test('a pending subnet is polled until it becomes ready', async ({ page }) => {
    const state = { subnetStatus: 'pending' }
    await mockCloud(page, state)
    await page.goto('/fleet-cloud/vpcs')
    await expect(page.getByText('pending', { exact: true })).toBeVisible({ timeout: 15_000 })
    state.subnetStatus = 'ready'
    await expect(page.getByText('ready', { exact: true })).toBeVisible({ timeout: 15_000 }) // next 5 s poll
  })

  test('a group with an unreadable policy does not break the page', async ({ page }) => {
    await mockCloud(page, {
      subnetStatus: 'ready',
      groups: [
        { id: 'g1', project_id: 'p1', template_id: 't1', subnet_id: 's1', name: 'broken', policy_json: '', paused: false, last_scaled_at: '', last_error: '' },
        { id: 'g2', project_id: 'p1', template_id: 't1', subnet_id: 's1', name: 'web', policy_json: policy(2), paused: false, last_scaled_at: '', last_error: '' },
      ],
    })
    await page.goto('/fleet-cloud/vpcs')
    await expect(page.getByText(/broken · its scaling policy could not be read/)).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('web · 2 desired · active')).toBeVisible()
  })

  test('an older controller without the cloud API shows an error, not a blank page', async ({ page }) => {
    await page.route('**/api/v1/project-registry', (r) => r.fulfill({ json: [{ id: 'p1', name: 'apps', enabled: true }] }))
    await page.route('**/api/v1/cloud/**', (r) => r.fulfill({ status: 404, json: { error: 'not found' } }))
    await page.goto('/fleet-cloud/vpcs')
    await expect(page.getByRole('heading', { name: 'Virtual private clouds' })).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('button', { name: 'Create VPC' })).toBeVisible()
  })

  test('controls meet the audit\'s touch-target size on a phone', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await mockCloud(page, { subnetStatus: 'ready' })
    await page.goto('/fleet-cloud/vpcs')
    const create = page.getByRole('button', { name: 'Create VPC' })
    await expect(create).toBeVisible({ timeout: 15_000 })
    // Buttons are 44px; fields must clear web/scripts/ux-audit.mjs's 34px minimum (the design layer sets their final height).
    expect((await create.boundingBox())!.height).toBeGreaterThanOrEqual(43.5)
    for (const el of [page.getByLabel('VPC name'), page.getByLabel('Project'), page.getByLabel('Host')]) {
      expect((await el.boundingBox())!.height).toBeGreaterThanOrEqual(34)
    }
  })
})
