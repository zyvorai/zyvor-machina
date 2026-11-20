// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('Platform guest UX', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
  })

  test('guest health tab shows error banner on API failure', async ({ page }) => {
    await page.goto('/platform/vms/guest-health-fail?tab=guestHealth')
    await expect(page.getByText('Could not load guest health')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible()
  })

  test('fleet analyze guests card is dismissible and supports empty matches', async ({ page }) => {
    await page.route('**/*fleet/guest-query*', async (route) => {
      if (route.request().method() !== 'POST') {
        await route.continue()
        return
      }
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'empty-test',
          summary: 'No VMs matched for empty-test query.',
          matched_count: 0,
          scanned_count: 3,
          llm_powered: false,
          vms: [],
        }),
      })
    })
    await page.goto('/platform/vms')
    await page.getByTestId('machine-finder-search').fill('empty-test')
    await page.getByRole('button', { name: 'Analyze' }).click()
    // The card's classes moved from literal Tailwind (rounded-xl, text-slate-200) to netra-look's
    // CSS-var tokens (rounded-2xl, text-[var(--text-primary)]) — match on text instead of color class.
    const report = page.getByTestId('machine-finder-page').locator('div.rounded-2xl').filter({
      has: page.getByText('No VMs matched for empty-test query.'),
    })
    await expect(report).toBeVisible({ timeout: 15_000 })
    await report.locator('button[aria-label="Dismiss"]').evaluate((btn) => (btn as HTMLButtonElement).click())
    await expect(report).toHaveCount(0, { timeout: 5000 })
  })

  test('fleet guest query VM rows deep-link to guest health tab', async ({ page }) => {
    await page.route('**/*fleet/guest-query*', async (route) => {
      if (route.request().method() !== 'POST') {
        await route.continue()
        return
      }
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          query: 'guest agent',
          summary: '1 VM matched.',
          matched_count: 1,
          scanned_count: 1,
          llm_powered: false,
          vms: [{ vm_id: 'v1', vm_name: 'vm-1', os_pretty_name: 'Ubuntu', guest_ip: '10.0.0.5', install_state: 'running', user_count: 1, flags: [] }],
        }),
      })
    })
    await page.goto('/platform/vms')
    await page.getByRole('button', { name: 'Analyze' }).click()
    await page.locator('a[href="/platform/vms/v1?tab=guestHealth"]').click()
    await expect(page).toHaveURL(/\/platform\/vms\/v1\?tab=guestHealth/)
  })
})
