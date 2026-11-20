// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { expandFleetInsights } from './helpers/platformTestHelpers'
import { mockPlatformApi } from './platformMock'

test('Infrastructure DNA strip visible on dashboard (power tier)', { retries: 1 }, async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expandFleetInsights(page)
  const strip = page.getByTestId('infrastructure-dna-strip')
  await expect(strip).toBeVisible({ timeout: 15_000 })
  await expect(strip.getByText('Infrastructure DNA', { exact: true })).toBeVisible()
  await expect(strip.getByLabel(/Infrastructure DNA score 88/i)).toBeVisible()
  await expect(strip.getByText(/Grade/)).toBeVisible()
})
