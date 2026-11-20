// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Live Preview Wall shows running VM tiles and Open Cinema', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/mission-control/live')
  await expect(page.getByTestId('mission-control-live-wall')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('live-wall-tile-vm-1')).toBeVisible()
  await page.getByTestId('live-wall-tile-vm-1').getByTestId('live-wall-open-cinema').click()
  await expect(page).toHaveURL(/\/platform\/vms\/v1\/consolehub/)
  await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
})
