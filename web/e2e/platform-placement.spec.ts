// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Placement recommendations offer manual migrate', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power', placementRecommendations: true })
  await page.goto('/platform/placement')
  await expect(page.getByText('vm-1')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Migrate' }).click()
  await expect(page.getByRole('heading', { name: /Live migrate vm-1/i })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('Target host: host-2')).toBeVisible()
})
