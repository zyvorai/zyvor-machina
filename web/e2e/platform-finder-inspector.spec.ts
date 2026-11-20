// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Machine Finder command center shows actions for selected VM', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/vms')
  await expect(page.getByRole('heading', { name: 'Machine Finder', exact: true })).toBeVisible({ timeout: 15_000 })
  // The Machine Finder list fixture names this VM "web-01" (id v1) — a differently-named fixture
  // backs its own detail page.
  await expect(page.getByText('web-01').first()).toBeVisible({ timeout: 15_000 })
  await page.getByText('web-01').first().click()
  const inspector = page.getByTestId('machine-finder-command-center')
  await expect(inspector).toBeVisible({ timeout: 10_000 })
  await expect(inspector.getByRole('link', { name: 'Open VM detail' })).toBeVisible()
  await expect(inspector.getByText('Command Center')).toBeVisible()
})

test('Machine Finder topology lens shows geography labels', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/vms?lens=topology')
  await expect(page.getByTestId('machine-finder-topology')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'host-1' }).click()
  await expect(page.getByRole('link', { name: 'Open host' })).toBeVisible({ timeout: 10_000 })
  // Renamed from platform-finder-inspector to tahoe-glass-card in the netra-look rewrite (3ddec01c).
  const inspector = page.locator('.tahoe-glass-card').first()
  await expect(inspector).toBeVisible()
  await expect(inspector.getByText('Site', { exact: true })).toBeVisible()
  // Two "web-01" buttons render once a host is selected (the VM chip row + the inspector's own VM
  // list) — both open the same VmInspector, so take the first.
  await page.getByRole('button', { name: 'web-01', exact: true }).first().click()
  await expect(inspector.getByText('State', { exact: true })).toBeVisible()
  await expect(inspector.getByRole('link', { name: 'Open VM' })).toBeVisible()
})
