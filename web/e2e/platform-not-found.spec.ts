// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('platform unknown route shows recovery page', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/does-not-exist-route')
  await expect(page.getByTestId('platform-not-found')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('platform-not-found').getByRole('link', { name: 'Mission Control' })).toBeVisible()
})
