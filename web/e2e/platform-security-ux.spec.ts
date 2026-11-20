// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('platform security UX', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
  })

  test('/platform/zeus/security/ports shows briefing stats', async ({ page }) => {
    await page.goto('/platform/zeus/security/ports')
    await expect(page.getByTestId('platform-firewall-ports-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('heading', { level: 1, name: 'Open Ports' })).toBeVisible()
  })

  test('/platform/zeus/security/services shows services panel', async ({ page }) => {
    await page.goto('/platform/zeus/security/services')
    await expect(page.getByTestId('platform-firewall-services-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('Allowed services')).toBeVisible()
  })

  test('/platform/zeus/security/activity shows empty state when no events', async ({ page }) => {
    await page.route('**/zeus-firewall/**/activity**', async (route) => {
      await route.fulfill({ json: { events: [], note: 'Mock empty' } })
    })
    await page.goto('/platform/zeus/security/activity')
    await expect(page.getByTestId('platform-firewall-activity-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('No connection events yet')).toBeVisible()
  })
})
