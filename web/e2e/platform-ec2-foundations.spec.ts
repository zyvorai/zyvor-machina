// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const FLAVORS = [
  { id: 'f-small', name: 'small', vcpus: 1, memory_mib: 2048, disk_gib: 20, description: '', is_public: true },
  { id: 'f-large', name: 'large', vcpus: 4, memory_mib: 8192, disk_gib: 80, description: '', is_public: true },
]

test.describe('EC2-style foundations', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page)
    await page.route('**/api/v1/flavors', (r) => r.fulfill({ json: FLAVORS }))
  })

  test('an instance shows its EC2-style id, can be tagged and untagged', async ({ page }) => {
    const store: Record<string, string> = { env: 'prod' }
    await page.route('**/api/v1/tags/vm/*', async (r) => {
      const req = r.request()
      if (req.method() === 'PUT') Object.assign(store, JSON.parse(req.postData() ?? '{}').tags)
      if (req.method() === 'DELETE') for (const k of JSON.parse(req.postData() ?? '{}').keys) delete store[k]
      await r.fulfill({ json: { resource_type: 'vm', id: 'vm-1', ec2_id: 'i-0123456789abcdef0', tags: { ...store } } })
    })
    await page.goto('/fleet-cloud/instances/vm-1')
    const editor = page.getByTestId('tag-editor')
    await expect(editor).toBeVisible({ timeout: 15_000 })
    await expect(editor).toContainText('i-0123456789abcdef0')
    await expect(editor.locator('[data-tag="env"]')).toBeVisible()
    await editor.getByLabel('Tag key').fill('owner')
    await editor.getByLabel('Tag value').fill('sam')
    await editor.getByRole('button', { name: 'Add tag' }).click()
    await expect(editor.locator('[data-tag="owner"]')).toBeVisible()
    await editor.getByRole('button', { name: 'Remove tag env' }).click()
    await expect(editor.locator('[data-tag="env"]')).toHaveCount(0)
    expect(store).toEqual({ owner: 'sam' })
  })

  test('changing the instance type asks first and then queues the change', async ({ page }) => {
    const calls: string[] = []
    await page.route('**/api/v1/tags/**', (r) => r.fulfill({ status: 404, json: { error: 'not found' } }))
    await page.route('**/api/v1/vms/*/change-type', async (r) => {
      calls.push(r.request().postData() ?? '')
      await r.fulfill({ json: { task_id: 't', status: 'pending', operation: 'vm.change_type' } })
    })
    await page.goto('/fleet-cloud/instances/vm-1')
    const card = page.getByTestId('instance-type')
    await expect(card).toBeVisible({ timeout: 15_000 })
    await card.getByLabel('New instance type').selectOption('f-large')
    await card.getByRole('button', { name: 'Change type…' }).click()
    await expect(card.getByRole('group', { name: 'Confirm instance type change' })).toContainText('cleanly')
    expect(calls).toHaveLength(0) // nothing is sent until the user confirms
    await card.getByRole('button', { name: /Shut down and resize|Resize/ }).click()
    await expect.poll(() => calls.length).toBe(1)
    expect(JSON.parse(calls[0])).toEqual({ flavor_id: 'f-large' })
    await expect(card.getByText(/Change queued/)).toBeVisible()
  })

  test('the tag editor stays out of the way on a controller without the tags API', async ({ page }) => {
    await page.route('**/api/v1/tags/**', (r) => r.fulfill({ status: 404, json: { error: 'not found' } }))
    await page.goto('/fleet-cloud/instances/vm-1')
    await expect(page.getByTestId('instance-type')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('tag-editor')).toHaveCount(0)
  })
})
