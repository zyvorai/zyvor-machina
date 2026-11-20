// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Migration radar OVF source opens the import wizard', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/migration')
  await page.getByRole('button', { name: /OVF \/ OVA File/ }).click()
  await expect(page).toHaveURL(/\/import$/, { timeout: 15_000 })
})
