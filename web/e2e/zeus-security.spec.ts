// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Security Center loads with threat score', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zeus/security')
  await expect(page.getByRole('heading', { name: 'Security Center' })).toBeVisible({ timeout: 15_000 })
  // The old "Fleet threat score" stat-widget label was dropped in the apple.com Story/Browse/Work
  // redesign in favor of a compact "Threat {score}" subtitle pill (PlatformSecurityCenter.tsx).
  await expect(page.getByText(/Threat \d+/)).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/4 nodes/)).toBeVisible({ timeout: 15_000 })
})

test('Machine Security view shows process tabs', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/machines/h1')
  await expect(page.getByRole('heading', { name: 'Machine security' })).toBeVisible({ timeout: 15_000 })
  // The "More" overflow button is a sibling of the tab strip's role="tablist", not a descendant of
  // it (DetailTabs.tsx) — scope to the sticky tab-strip container instead.
  const tabBar = page.locator('#platform-detail-tabs')
  await expect(tabBar.getByRole('tab', { name: 'Processes' })).toBeVisible()
  await tabBar.getByRole('button', { name: 'More' }).click()
  await page.getByRole('menuitem', { name: 'Containers' }).click()
  await expect(page.getByText('ns/zeus')).toBeVisible({ timeout: 15_000 })
})

test('Runtime enforcement workspace loads', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/enforcement')
  await expect(page.getByRole('heading', { name: 'Runtime enforcement' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Block reverse-shell listeners')).toBeVisible()
  await expect(page.getByText('Target hosts')).toBeVisible()
  await page.getByRole('button', { name: 'Preview' }).first().click()
  await expect(page.getByText('TracingPolicy preview')).toBeVisible({ timeout: 10_000 })
})

test('Threat hunting workspace loads', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/hunt')
  await expect(page.getByRole('heading', { name: 'Threat hunting' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Threat correlations')).toBeVisible()
  await page.getByRole('button', { name: 'Generate' }).click()
  await expect(page.getByText(/correlation finding/)).toBeVisible({ timeout: 15_000 })
})
