// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('platform observability UX', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
  })

  test('/platform/observability shows lens tabs', async ({ page }) => {
    await page.goto('/platform/observability')
    await expect(page.getByTestId('platform-observability-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('tab', { name: 'SLOs' })).toBeVisible()
    await expect(page.getByRole('tab', { name: 'Traces' })).toBeVisible()
    await page.getByRole('tab', { name: 'Traces' }).click()
    await expect(page.getByRole('tab', { name: 'Traces' })).toHaveAttribute('aria-selected', 'true')
  })

  test('/platform/events shows log stream with source tabs', async ({ page }) => {
    await page.goto('/platform/events')
    await expect(page.getByTestId('platform-events-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('tab', { name: 'Audit' })).toBeVisible()
    await expect(page.getByText('Log stream')).toBeVisible()
  })
})
