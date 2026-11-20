// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('vm detail shows daily access strip with connect copy ports export', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
  // VmConnectHub renders "Connect" as a plain eyebrow <p>, not a heading, on the overview tab.
  await expect(page.getByTestId('vm-daily-access').getByText('Connect', { exact: true })).toBeVisible()
  // Console is a primary tab
  await expect(page.getByRole('tab', { name: 'Console' })).toBeVisible()
  // Spec / Spec+XML export buttons live on the Access tab
  await page.getByRole('tab', { name: 'Access' }).click()
  await expect(page.getByRole('button', { name: 'Spec', exact: true }).first()).toBeVisible({ timeout: 5_000 })
  await expect(page.getByRole('button', { name: 'Spec + XML', exact: true }).first()).toBeVisible()
})

test('vm detail shows lifecycle power actions and SSH when guest IP present', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
  // Shutdown is the primary power button (always visible for running VMs)
  await expect(page.getByRole('button', { name: 'Shutdown' })).toBeVisible()
  // Pause and Force stop live in the "Power & more" dropdown
  await page.getByRole('button', { name: 'Power & more' }).click()
  await expect(page.getByRole('button', { name: 'Pause' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Force stop' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByText('ubuntu@192.168.122.50').first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'SSH', exact: true }).first()).toBeVisible()
})

test('create vm wizard shows auto-fetch when golden image missing', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.route('**/api/v1/templates/**/readiness', async (route) => {
    await route.fulfill({
      json: {
        disk_exists: false,
        host_online: 1,
        cloud_init: true,
        ready: true,
        auto_fetch: true,
        remediation: 'Golden image not on host yet — will download automatically on first create.',
        source_disk: '/var/lib/libvirt/images/photon-os.qcow2',
      },
    })
  })
  await page.goto('/platform/vms?create=photon-test')
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByText('Will download on first create')).toBeVisible({ timeout: 10_000 })
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.locator('button.btn-primary.min-w-\\[7rem\\]').filter({ hasText: /^Create VM$/ })).toBeEnabled()
})

test('create vm wizard shows readiness when template selected', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms?create=test-vm')
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByText('Ready to deploy')).toBeVisible({ timeout: 10_000 })
})

test('machine finder delete from command center does not crash', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  let vmGone = false
  await mockPlatformApi(page, { tier: 'power' })
  // after deletion, load() refetches the list — return empty list so the card disappears
  await page.route('**/platform/controller/api/v1/vms?*', async (route) => {
    if (vmGone && route.request().method() === 'GET') {
      return route.fulfill({ json: [] })
    }
    await route.fallback()
  })
  await page.route('**/platform/controller/api/v1/vms/v1', async (route) => {
    if (vmGone && route.request().method() === 'GET') {
      return route.fulfill({
        status: 404,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'not found', error_code: 'not_found' }),
      })
    }
    await route.fallback()
  })
  await page.route('**/platform/controller/api/v1/vms/v1/delete', async (route) => {
    vmGone = true
    return route.fulfill({
      json: { task_id: 'task-delete-mock', status: 'pending', operation: 'vm.delete' },
    })
  })

  await page.goto('/platform/vms')
  await expect(page.getByRole('heading', { name: 'Machine Finder', exact: true })).toBeVisible({
    timeout: 15_000,
  })

  // draggable divs swallow Playwright synthetic mouse events; dispatch a native DOM click instead
  await page.evaluate(() => {
    const el = document.querySelector('[data-testid="machine-card-v1"]')
    el?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, view: window }))
  })
  await expect(page.getByRole('heading', { name: 'Command Center' })).toBeVisible({ timeout: 15_000 })

  const deleteReq = page.waitForResponse(
    (r) => r.url().includes('/vms/v1/delete') && r.request().method() === 'POST',
  )
  await page.getByTestId('machine-finder-command-center').getByRole('button', { name: 'Delete' }).click()
  // ConfirmDialog (React modal) replaces browser confirm — click the dialog's confirm button
  await page.getByRole('dialog').getByRole('button', { name: 'Delete' }).click()
  expect((await deleteReq).ok()).toBeTruthy()

  await expect(page.getByTestId('machine-card-v1')).toHaveCount(0, { timeout: 10_000 })
  await expect(page.getByText(/Application error|Something went wrong/i)).toHaveCount(0)
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
})

test('delete vm returns to list without page crash', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  let vmGone = false
  await mockPlatformApi(page, { tier: 'power' })
  await page.route('**/platform/controller/api/v1/vms/v1', async (route) => {
    if (vmGone && route.request().method() === 'GET') {
      return route.fulfill({
        status: 404,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'not found', error_code: 'not_found' }),
      })
    }
    await route.fallback()
  })
  await page.route('**/platform/controller/api/v1/vms/v1/delete', async (route) => {
    vmGone = true
    return route.fulfill({
      json: { task_id: 'task-delete-mock', status: 'pending', operation: 'vm.delete' },
    })
  })

  await page.goto('/platform/vms/v1')
  await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })

  const deleteReq = page.waitForResponse(
    (r) => r.url().includes('/vms/v1/delete') && r.request().method() === 'POST',
  )
  // Delete VM is in the "Power & more" dropdown
  await page.getByRole('button', { name: 'Power & more' }).click()
  await page.getByRole('button', { name: 'Delete VM' }).click()
  // React ConfirmDialog — click the dialog's confirm button
  await page.getByRole('dialog').getByRole('button', { name: 'Delete' }).click()
  expect((await deleteReq).ok()).toBeTruthy()

  await expect(page).toHaveURL(/\/platform\/vms\/?$/, { timeout: 15_000 })
  await expect(page.getByText(/Application error|Something went wrong/i)).toHaveCount(0)
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
})

test('create vm wizard has Next steps and SSH on final step', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms?create=test-vm')
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Next' })).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('button', { name: /Ubuntu 24\.04/ }).first()).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByText('Custom size')).toBeVisible()
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByText('SSH public key (optional)')).toBeVisible()
  const pubkey = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI test@e2e'
  await page.getByPlaceholder('ssh-ed25519 AAAA').fill(pubkey)
  await expect(page.getByPlaceholder('ssh-ed25519 AAAA')).toHaveValue(pubkey)
})
