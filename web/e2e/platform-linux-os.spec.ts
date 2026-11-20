// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('host Linux tab shows PSI and package preview', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/hosts/h1?tab=linux')
  await expect(page.getByRole('heading', { name: 'host-1' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('tab', { name: 'Linux' }).click()
  await expect(page.getByText('Pressure stall (PSI)')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Preview upgrade', exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'Preview upgrade', exact: true }).click()
  await expect(page.getByText('Upgrade preview ready')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText(/Inst libc6/i)).toBeVisible({ timeout: 10_000 })
})

test('maintenance mission apply upgrades queues task', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/maintenance?tab=mission')
  await expect(page.getByText('Apply preview: ready')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Apply upgrades' }).click()
  // ConfirmDialog (React modal) — click the "Apply" confirm button
  await page.getByRole('dialog').getByRole('button', { name: 'Apply' }).click()
  await expect(page.getByText('Package upgrade queued')).toBeVisible({ timeout: 10_000 })
})

test('host Linux tab shows maintenance gate when not in maintenance', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power', hostMaintenanceMode: false })
  await page.goto('/platform/hosts/h1?tab=linux')
  await expect(page.getByRole('heading', { name: 'host-1' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Maintenance mode required')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Apply upgrade' })).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Enter maintenance' })).toBeVisible()
})

test('maintenance mission shows blocked step when host not in maintenance', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power', hostMaintenanceMode: false })
  await page.goto('/platform/maintenance?tab=mission')
  await expect(page.getByText('Step blocked — enter maintenance')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Apply upgrades' })).toBeDisabled()
})

test('Zeus OS fleet linux-health card visible', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra?tab=fleet')
  await expect(page.getByText('Fleet Linux health')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Pressure hosts')).toBeVisible()
  await expect(page.getByText('1 host(s) scanned')).toBeVisible()
})
