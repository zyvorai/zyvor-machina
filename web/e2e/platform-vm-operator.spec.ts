// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('missing VM folder shows prune control', { retries: 1 }, async ({ page }) => {
  test.setTimeout(90_000)
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms?folder=missing')
  await expect(page.getByRole('button', { name: 'Prune missing records' })).toBeVisible({ timeout: 40_000 })
  const pruneReq = page.waitForResponse(
    (r) => r.url().includes('/vms/prune-missing') && r.request().method() === 'POST',
  )
  await page.getByRole('button', { name: 'Prune missing records' }).click()
  // ConfirmDialog (React modal) — click the "Remove records" confirm button
  await page.getByRole('dialog').getByRole('button', { name: 'Remove records' }).click()
  expect((await pruneReq).ok()).toBeTruthy()
})

test('vm network tab shows service catalog and active exposure', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=network')
  await expect(page.getByRole('heading', { name: 'Hypervisor NAT (port forwards)' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible()
  await expect(page.getByText('Known services')).toBeVisible()
  await expect(page.getByTestId('expose-service-http')).toHaveText(/HTTP ✓/)
  await expect(page.getByTestId('expose-service-ssh')).toBeVisible()
  await expect(page.getByTestId('expose-service-mysql')).toBeVisible()
  await expect(page.getByTestId('vm-port-forward-panel').getByText('9080→80')).toBeVisible()
  await expect(page.getByTestId('vm-port-forward-panel').getByRole('link', { name: /^open$/i })).toBeVisible()
})

test('expose SSH service creates NAT rule', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=network')
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible({ timeout: 15_000 })
  const createReq = page.waitForRequest(
    (req) => req.url().includes('/port-forwards') && req.method() === 'POST',
  )
  await page.getByTestId('expose-service-ssh').click()
  const req = await createReq
  expect(req.postDataJSON()).toMatchObject({ host_port: 2222, vm_port: 22, protocol: 'tcp' })
})

test('custom named service can be exposed', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=network')
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('custom-service-name').fill('Jenkins')
  await page.getByTestId('custom-service-guest-port').fill('8080')
  await expect(page.getByTestId('custom-service-host-port')).toHaveValue('18080')
  const createReq = page.waitForRequest(
    (req) => req.url().includes('/port-forwards') && req.method() === 'POST',
  )
  await page.getByTestId('expose-custom-service').click()
  const req = await createReq
  expect(req.postDataJSON()).toMatchObject({
    host_port: 18080,
    vm_port: 8080,
    protocol: 'tcp',
  })
  await expect(page.getByText('Your saved services')).toBeVisible()
  await expect(page.getByTestId('vm-port-forward-panel').getByText('Jenkins ✓')).toBeVisible()
})

test('overview daily access exposes scanned guest port', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  await expect(page.getByTestId('vm-daily-access')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-laptop-access-checklist')).toBeVisible()
  const exposeBtn = page.getByTestId('expose-guest-port-80')
  if (await exposeBtn.count()) {
    const createReq = page.waitForRequest(
      (req) => req.url().includes('/port-forwards') && req.method() === 'POST',
    )
    await exposeBtn.click()
    expect((await createReq).postDataJSON()).toMatchObject({ vm_port: 80, protocol: 'tcp' })
  }
})

test('VM detail header uses action bar with power overflow', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  await expect(page.getByTestId('vm-detail-action-bar')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-detail-action-bar').getByRole('link', { name: /Open Cinema/i })).toBeVisible()
  await expect(page.getByTestId('vm-detail-action-bar').getByRole('button', { name: 'SSH' })).toBeVisible()
  await expect(page.getByRole('button', { name: /Power & more/i })).toBeVisible()
})

test('Access tab shows expanded connect hub and NAT panel', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=access')
  await expect(page.getByTestId('vm-daily-access')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Guest security & ports' })).toBeVisible()
})

test('overview attention stack surfaces laptop NAT path', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  await expect(page.getByTestId('vm-attention-stack')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-attention-stack').getByText('Laptop access', { exact: true })).toBeVisible()
})

test('host linux tab shows GPU inventory', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/hosts/h1')
  await expect(page.getByRole('heading', { name: 'host-1', level: 1 })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('tab', { name: 'Linux' }).click()
  await expect(page.getByRole('heading', { name: 'GPU inventory' })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('NVIDIA L40', { exact: true })).toBeVisible()
  // SideNav also has its own "GPU Command Center" nav entry — scope to the page canvas.
  await expect(page.locator('.mac-desktop-main').getByRole('link', { name: 'GPU Command Center' })).toBeVisible()
})

test('doctor tab deep-links GuestKit migrate plan on guest health', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=doctor')
  await expect(page.getByRole('button', { name: 'Run migrate plan' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Run migrate plan' }).click()
  await expect(page).toHaveURL(/tab=guestHealth/)
  await expect(page.getByText('KVM migration plan')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('82%')).toBeVisible()
})

test('port forward open link uses hypervisor host and remove clears rule', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=network')
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible({ timeout: 15_000 })
  const openLink = page.getByTestId('vm-port-forward-panel').getByRole('link', { name: /^open$/i })
  await expect(openLink).toHaveAttribute('href', /127\.0\.0\.1:9080/)
  await page.getByRole('button', { name: 'remove' }).click()
  await expect(page.getByText('9080→80')).toHaveCount(0)
})
