// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('GPU Command Center shows host inventory and placement advisor', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/gpu')
  await expect(page.getByRole('heading', { name: /GPU Command Center/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('table').getByRole('link', { name: 'host-1' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/CUDA placement advisor/i)).toBeVisible()
  await expect(page.getByText(/CUDA-ready/i)).toBeVisible()
  await page.getByRole('button', { name: 'Create VM here' }).first().click()
  await expect(page).toHaveURL(/\/platform\/vms\?create=gpu-workload/)
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({ timeout: 15_000 })
})

test('Host detail linux tab links to GPU Command Center', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/hosts/h1?tab=linux')
  await expect(page.getByRole('heading', { name: 'host-1' })).toBeVisible({ timeout: 15_000 })
  // SideNav also has its own "GPU Command Center" nav entry — scope to the page canvas to hit the
  // host-detail panel's inline link instead.
  await page.locator('.mac-desktop-main').getByRole('link', { name: 'GPU Command Center' }).click()
  await expect(page).toHaveURL(/\/platform\/gpu/)
  await expect(page.getByRole('heading', { name: /GPU Command Center/i })).toBeVisible({ timeout: 15_000 })
})
