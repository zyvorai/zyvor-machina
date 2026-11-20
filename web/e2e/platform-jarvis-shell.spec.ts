// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

// This file predates the shell unification (GlobalBar + SideNav, layouts/PlatformLayout.tsx).
// Two things it tested no longer exist: PlatformSidebar.tsx / PlatformJarvisBriefing.tsx are
// unimported dead code, and tier no longer hides the sidebar or swaps in a different dashboard
// shell — "Desktop tier no longer starves the menubar" (utils/platformMacMenus.ts) applies to the
// sidebar too. What each test actually verified — a briefing-chip action, the dashboard's search
// entry point, the Normal-tier launchpad's reduced density — is rewritten against current markup.

test('Normal tier dashboard shows a working briefing chip', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  // "Recovery" is the one briefing chip MissionControlBriefing.tsx always renders (the others are
  // conditional on fleet state); the top bar is present at every tier.
  await expect(page.getByRole('button', { name: 'Recovery' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('navigation', { name: 'Primary' })).toBeVisible()
})

test('Jarvis briefing chip navigates to its target route', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await page.getByRole('button', { name: 'Recovery' }).click()
  await expect(page).toHaveURL(/\/platform\/backups/, { timeout: 15_000 })
})

test('Jarvis shell search entry point visible on power tier dashboard', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  // Scoped to the GlobalBar: the briefing itself also has its own "Search ⌘K" chip.
  await expect(page.locator('.gnb-bar').getByRole('button', { name: 'Search' })).toBeVisible()
})

test('normal tier dashboard shows launchpad without fleet insights', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-launchpad')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('platform-fleet-insights-toggle')).toHaveCount(0)
})
