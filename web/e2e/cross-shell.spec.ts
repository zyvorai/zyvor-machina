// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('classic storage empty state stays inside the unified platform shell', async ({ page }) => {
  // Classic (non-/platform) routes and /platform/* routes now share one shell
  // (layouts/PlatformLayout.tsx, GlobalBar + SideNav) — there is no longer a separate "classic"
  // chrome to bridge back from, and ShellBridgeBar.tsx is unimported dead code. The GlobalBar's
  // brand mark is the shell's constant way home from anywhere, including this classic route.
  await mockPlatformApi(page, { tier: 'normal', emptyStorage: true })
  await page.goto('/storage')
  await expect(page.getByText('No storage pools')).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('.gnb-bar')).toBeVisible()
  await page.getByRole('link', { name: 'Machina home' }).click()
  await expect(page).toHaveURL(/\/platform\/?$/, { timeout: 20_000 })
})

test('classic navbar help opens platform guide dialog', async ({ page }) => {
  // Navbar.tsx (the old classic-only navbar with its own "Help menu" button) is unimported dead
  // code — /vms renders under the same PlatformLayout shell as /platform/vms, so help lives in
  // the same "Machina" dropdown (.gnb-brand button, .mac-menu-panel; items are role="menuitem").
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/vms')
  await expect(page.getByRole('heading', { name: /Virtual machines/i })).toBeVisible({ timeout: 15_000 })
  await page.locator('.gnb-brand button').click()
  await page.locator('.mac-menu-panel').getByRole('menuitem', { name: 'Platform guide…' }).click()
  await expect(page.getByRole('dialog', { name: 'Help' })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText(/libvirt\/KVM stays the engine/i)).toBeVisible()
})

test('platform help menu opens platform guide dialog', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/vms')
  await expect(page.getByRole('heading', { level: 1, name: 'Machine Finder' })).toBeVisible({
    timeout: 15_000,
  })
  // The old menubar had a separate "Help" menu; the current GlobalBar folds Platform guide /
  // Keyboard shortcuts / About into the "Machina" dropdown (.gnb-brand button, .mac-menu-panel
  // is unchanged). Items in it are role="menuitem" (see PlatformMenuItem.tsx), not "button".
  await page.locator('.gnb-brand button').click()
  await page.locator('.mac-menu-panel').getByRole('menuitem', { name: 'Platform guide…' }).click()
  await expect(page.getByRole('dialog', { name: 'Help' })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText(/libvirt\/KVM stays the engine/i)).toBeVisible()
})

test('platform VM detail links to ConsoleHub', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  // "Open Cinema" is the action bar link that routes to /consolehub
  await expect(page.getByRole('link', { name: 'Open Cinema' }).first()).toBeVisible({ timeout: 15_000 })
  await page.getByRole('link', { name: 'Open Cinema' }).first().click()
  await expect(page).toHaveURL(/\/consolehub/)
})
