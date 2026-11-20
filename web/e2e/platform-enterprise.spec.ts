// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Enterprise Keychain tab shows secrets inventory', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/enterprise?tab=keychain')
  await expect(page.getByRole('heading', { name: 'Enterprise Security' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('automation')).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Secrets inventory' })).toBeVisible()
})
