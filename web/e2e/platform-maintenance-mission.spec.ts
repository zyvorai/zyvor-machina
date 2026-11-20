// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Maintenance Mission shows 7-step timeline', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/maintenance?tab=mission')
  const timeline = page.getByRole('list', { name: 'Build progress' })
  await expect(timeline).toBeVisible({ timeout: 15_000 })
  await expect(timeline.getByText('Scan fleet', { exact: true })).toBeVisible()
  await expect(timeline.getByText('Assess risk', { exact: true })).toBeVisible()
  await expect(timeline.getByText('Schedule window', { exact: true })).toBeVisible()
  await expect(timeline.getByText('Enter maintenance', { exact: true })).toBeVisible()
  await expect(timeline.getByText('Evacuate VMs', { exact: true })).toBeVisible()
  await expect(timeline.getByText('Apply preview', { exact: true })).toBeVisible()
  await expect(timeline.getByText('Verify & exit', { exact: true })).toBeVisible()
})

test('Maintenance schedule enqueue shows ErrorBanner on failure', { retries: 1 }, async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/maintenance?tab=schedules')
  // Wait for hosts to load so the select has options before we try to use it
  await expect(page.locator('select[aria-label="Host"] option:not([value=""])')).toBeAttached({ timeout: 15_000 })
  await page.locator('select[aria-label="Host"]').selectOption('h1')
  const future = new Date(Date.now() + 86_400_000)
  const local = new Date(future.getTime() - future.getTimezoneOffset() * 60_000)
    .toISOString()
    .slice(0, 16)
  await page.locator('input[type="datetime-local"]').fill(local)

  await page.route('**/api/v1/maintenance/schedules', async (route) => {
    if (route.request().method() === 'POST') {
      return route.fulfill({ status: 500, json: { error: 'schedule failed' } })
    }
    return route.continue()
  })

  await page.getByTestId('schedule-maintenance-btn').click()
  await expect(page.getByRole('alert')).toContainText(/schedule failed|failed/i, { timeout: 10_000 })
})
