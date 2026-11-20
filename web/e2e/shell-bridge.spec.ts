// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'
import { waitForPlatformSession } from './helpers'

// ShellBridgeBar.tsx (the "Back to Platform" / "Apps & Integrations" banner these tests were
// written against) is unimported dead code — classic (non-/platform) routes and /platform/* routes
// share the same shell now (GlobalBar + SideNav, layouts/PlatformLayout.tsx), so there's no longer
// a separate "classic" chrome to bridge back from. The GlobalBar's brand mark (`aria-label="Machina
// home"`) is the shell's constant way home from anywhere, including these classic routes.

test('classic import route stays inside the unified platform shell', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/import')
  await waitForPlatformSession(page)
  await expect(page.locator('.gnb-bar')).toBeVisible({ timeout: 20_000 })
  await expect(page.getByRole('link', { name: 'Machina home' })).toBeVisible()
})

test('classic K8s route links back to platform via the shell brand mark', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/k8s')
  await waitForPlatformSession(page)
  await expect(page.locator('.gnb-bar')).toBeVisible({ timeout: 20_000 })
  await page.getByRole('link', { name: 'Machina home' }).click()
  await expect(page).toHaveURL(/\/platform\/?$/, { timeout: 20_000 })
})

test('classic storage route stays inside the unified platform shell', async ({ page }) => {
  // The old shell bridge bar's dedicated "Apps & Integrations" quick link no longer has an
  // equivalent — Integrations now lives under Settings (PlatformSettingsHub.tsx), reachable via
  // the Machina menu's "Settings" item like everywhere else, not a classic-route-only shortcut.
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/storage')
  await waitForPlatformSession(page)
  await expect(page.locator('.gnb-bar')).toBeVisible({ timeout: 20_000 })
  await expect(page.getByRole('navigation', { name: 'Primary' })).toBeVisible()
})

test('Fleet Cloud stays inside the unified platform shell', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/fleet-cloud')
  await waitForPlatformSession(page)
  await expect(page.locator('.gnb-bar')).toBeVisible({ timeout: 20_000 })
  await page.getByRole('link', { name: 'Machina home' }).click()
  await expect(page).toHaveURL(/\/platform\/?$/, { timeout: 20_000 })
})

test('classic storage shows empty state when no pools', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal', emptyStorage: true })
  await page.goto('/storage')
  await expect(page.getByText('No storage pools')).toBeVisible({ timeout: 15_000 })
})
