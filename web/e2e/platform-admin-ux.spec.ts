// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('platform admin UX', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
  })

  test('/platform/users shows briefing and user table', async ({ page }) => {
    await page.goto('/platform/users')
    await expect(page.getByTestId('platform-users-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('heading', { name: /Access & Workspaces/i })).toBeVisible()
    await expect(page.getByTestId('platform-users-page').getByText(/1 user · 1 admin/i)).toBeVisible()
    await expect(page.getByRole('row', { name: /admin/i })).toBeVisible()
  })

  test('/platform/api-keys shows empty state when no keys', async ({ page }) => {
    await page.route('**/api/v1/api-keys**', async (route) => {
      await route.fulfill({ json: [] })
    })
    await page.goto('/platform/api-keys')
    await expect(page.getByTestId('platform-api-keys-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('No API keys yet')).toBeVisible()
  })

  test('/platform/api-keys lists keys when present', async ({ page }) => {
    await page.goto('/platform/api-keys')
    await expect(page.getByTestId('platform-api-keys-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('table').getByText('automation')).toBeVisible()
  })

  test('settings webhooks section shows empty endpoints', async ({ page }) => {
    await page.goto('/platform/settings?section=webhooks')
    await expect(page.getByTestId('platform-webhooks-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('No webhook endpoints')).toBeVisible()
  })

  test('/platform/projects shows spaces briefing', async ({ page }) => {
    // "Spaces strip" was the title of a MacGlassPanel that no longer exists — the page dropped it
    // in the shell rewrite and now shows the summary line + stats directly in PlatformPageChrome's
    // subtitle (see PlatformProjects.tsx), with the table beneath.
    await page.goto('/platform/projects')
    await expect(page.getByTestId('platform-projects-page')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('1 space · 2 VMs')).toBeVisible()
    await expect(page.getByRole('table').getByText('default')).toBeVisible()
  })
})
