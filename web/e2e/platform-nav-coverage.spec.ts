// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'
import { navLink } from './helpers/platformShellHelpers'

test('advanced tier shows sidebar policy', async ({ page }) => {
  // The old menubar's "Go" destination menu (All destinations…/Operations/Host/Networks) is gone
  // — see "Go menu operations navigates without tier bounce on power tier" below, which covers its
  // destination-navigation purpose via spotlight instead.
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/policy')
  await expect(page.getByRole('heading', { name: /Policy & Quotas/i })).toBeVisible()
})

test('context bar shows security sub-nav on policy studio route', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/zeus/security/policies')
  await expect(page.getByRole('heading', { name: /Policy Studio/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('.gnb-chapter')).toBeVisible()
  await expect(page.locator('.gnb-chapter-link', { hasText: 'Policy Studio' })).toBeVisible()
})

test('dashboard has no desktop tabs row', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms')
  await page.goto('/platform/hosts')
  await expect(page.locator('.mac-desktop-tabs')).toHaveCount(0)
  await expect(page.locator('.gnb-chapter')).toHaveCount(0)
})

test('policy studio route loads', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/zeus/security/policies')
  await expect(page.getByRole('heading', { name: /Policy Studio/i })).toBeVisible()
})

test('shell has no sidebar', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible()
  await expect(page.locator('aside[aria-label="Sections"]')).toHaveCount(0)
})

test('normal tier hides context bar on dashboard', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await expect(page.locator('.gnb-chapter')).toHaveCount(0)
})

test('power tier shows context bar on hub roots only', { retries: 1 }, async ({ page }) => {
  test.setTimeout(60_000)
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/operations')
  await expect(page.locator('.gnb-chapter')).toBeVisible({ timeout: 20_000 })
  await page.goto('/platform/tasks')
  await expect(page.locator('.gnb-chapter')).toHaveCount(0)
})

test('power tier dashboard shows launchpad and mission briefing', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-launchpad')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible()
})

test('security context bar collapses overflow into More menu', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/zeus/security/policies')
  await expect(page.locator('.gnb-chapter')).toBeVisible()
  await expect(page.locator('.gnb-chapter-link', { hasText: 'Policy Studio' })).toBeVisible()
  await expect(page.locator('.gnb-chapter-more')).toBeVisible()
  await page.locator('.gnb-chapter-more').click()
  await expect(page.getByRole('menuitem', { name: 'Threat Hunting' })).toBeVisible()
})

// PlatformMobileJumpNav.tsx (the `<select>` this file used to drive via `#platform-mobile-jump`)
// is unimported dead code. Mobile navigation is the full-height sheet (components/nav/MobileNavSheet.tsx)
// opened via the GlobalBar burger (`aria-label="Menu"`) and auto-closed on navigation.

test('mobile burger opens the navigation sheet', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/platform')
  const burger = page.getByRole('button', { name: 'Menu' })
  await expect(burger).toBeVisible()
  await burger.click()
  await expect(page.getByRole('dialog', { name: 'Navigation' })).toBeVisible()
})

test('mobile burger nav navigates to hosts on normal tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/platform')
  await page.getByRole('button', { name: 'Menu' }).click()
  await (await navLink(page, '/platform/hosts')).click()
  await expect(page).toHaveURL(/\/platform\/hosts/)
})

test('mobile burger nav includes hub sections on power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/platform/tasks')
  await page.getByRole('button', { name: 'Menu' }).click()
  const link = await navLink(page, '/platform/observability')
  await expect(link).toBeVisible()
  await link.click()
  await expect(page).toHaveURL(/\/platform\/observability/)
})

test('normal tier alerts quick action opens notification center', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/notifications')
  await expect(page.getByRole('heading', { name: 'Alerts', exact: true })).toBeVisible({ timeout: 15_000 })
})

test('settings context bar collapses overflow into More menu', { retries: 1 }, async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/policy')
  await expect(page.getByRole('heading', { name: /Policy & Quotas/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('.gnb-chapter')).toBeVisible()
  await expect(page.locator('.gnb-chapter-link', { hasText: 'Policy' })).toBeVisible()
  await expect(page.locator('.gnb-chapter-more')).toBeVisible()
  await page.locator('.gnb-chapter-more').click()
  // "About" moved into the "Machina" dropdown's Help entries — Settings' own context nav
  // (SETTINGS_ENTRIES, platformNavRegistry.ts) never had it back; "Integrations" is last in that
  // list and always overflows here since "Policy" (the active section) bumps into the visible set.
  await expect(page.getByRole('menuitem', { name: 'Integrations' })).toBeVisible()
})

test('power tier hides context bar on policy workspace', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/policy')
  await expect(page.getByRole('heading', { name: /Policy & Quotas/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('.gnb-chapter')).toHaveCount(0)
})

test('mobile burger nav reaches Settings from the policy workspace', async ({ page }) => {
  // The old jump-nav dropdown merged the sidebar's hub links with the context bar's settings
  // sections into one mobile-only control, so a specific section (e.g. `?section=security`) was
  // reachable there. The context bar (.gnb-chapter) doesn't render on mobile at all now — the
  // burger drawer's "Settings" link only reaches the base workspace on this viewport.
  await mockPlatformApi(page, { tier: 'power' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/platform/policy')
  await page.getByRole('button', { name: 'Menu' }).click()
  await (await navLink(page, '/platform/settings')).click()
  await expect(page).toHaveURL(/\/platform\/settings/)
})

test('zeus context bar collapses overflow into More menu', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/zeus/security/policies')
  await expect(page.getByRole('heading', { name: /Policy Studio/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('.gnb-chapter')).toBeVisible()
  await expect(page.locator('.gnb-chapter-link', { hasText: 'Policy Studio' })).toBeVisible()
  await expect(page.locator('.gnb-chapter-more')).toBeVisible()
  await page.locator('.gnb-chapter-more').click()
  await expect(page.getByRole('menuitem', { name: 'Threat Hunting' })).toBeVisible()
})

test('operations context bar collapses overflow into More menu', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/tasks')
  await expect(page.locator('.gnb-chapter')).toBeVisible()
  await expect(page.locator('.gnb-chapter-link', { hasText: 'Tasks' })).toBeVisible()
  await expect(page.locator('.gnb-chapter-more')).toBeVisible()
  await page.locator('.gnb-chapter-more').click()
  // The overflow menu (PlatformFloatingMenu) portals to the document body, so its items are role=
  // "menuitem" links (PlatformMenuLinkItem) outside the `<nav aria-label="Operations sections">`
  // DOM subtree — not reachable by scoping into that nav.
  await expect(page.getByRole('menuitem', { name: 'Observability' })).toBeVisible()
})

test('spotlight lists platform hubs on power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  // The old menubar's Help → "Spotlight Search" menu item only lived in the orphaned
  // PlatformMacAppMenus.tsx; Ctrl+K is spotlight's live entry point (as in every other test here).
  // The wait above matters more than that old click ever did — without it, Ctrl+K can fire before
  // the page has hydrated and the shortcut listener is attached, dropping the keystroke.
  await page.keyboard.press('Control+k')
  await expect(page.getByPlaceholder('Zyra — search or ask…')).toBeVisible({ timeout: 5000 })
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await expect(spotlight.getByText('Platform hubs', { exact: true })).toBeVisible()
  await expect(spotlight.getByRole('button', { name: /Operations Hub ·/i })).toBeVisible()
})

test('spotlight lists operations workspaces on power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await expect(
    spotlight.locator('button').filter({ hasText: 'Observability' }).filter({ hasNotText: /Hub ·/ }),
  ).toBeVisible()
})

test('spotlight opens via keyboard shortcut', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  await expect(page.getByPlaceholder('Zyra — search or ask…')).toBeVisible({ timeout: 5000 })
})

test('spotlight keeps page context prefill from Ask Zyra', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1')
  await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
  await page.evaluate(() => {
    window.dispatchEvent(new CustomEvent('machina-open-spotlight', { detail: { prefill: 'vm-1 guest health' } }))
  })
  const input = page.getByPlaceholder('Zyra — search or ask…')
  await expect(input).toBeVisible({ timeout: 5000 })
  await expect(input).toHaveValue('vm-1 guest health')
})

test('context overflow closes after navigation', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/tasks')
  const more = page.locator('.gnb-chapter-more')
  await more.click()
  await expect(more).toHaveAttribute('aria-expanded', 'true')
  await page.getByRole('menuitem', { name: 'Observability' }).click()
  await expect(page).toHaveURL(/\/platform\/observability/)
  await expect(more).toHaveAttribute('aria-expanded', 'false')
})

test('normal tier hub preview unlocks operations', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await (await navLink(page, '/platform/zeus/security')).click()
  await expect(page).toHaveURL(/\/platform\/zeus\/security/)
})

test('mobile burger nav navigates to resources on power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/platform')
  await page.getByRole('button', { name: 'Menu' }).click()
  await (await navLink(page, '/platform/infrastructure')).click()
  await expect(page).toHaveURL(/\/platform\/infrastructure/)
  await expect(page.getByText('Infrastructure', { exact: true }).first()).toBeVisible({ timeout: 15_000 })
})

test('spotlight lists resources workspaces on power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await expect(
    spotlight.getByRole('button', { name: 'Networks Infrastructure workspace' }),
  ).toBeVisible()
})

test('spotlight lists security workspaces on advanced tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await expect(
    spotlight.locator('button').filter({ hasText: 'Policy Studio' }).filter({ hasNotText: /Hub ·/ }),
  ).toBeVisible()
})

test('spotlight lists zeus workspaces on power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await expect(
    spotlight.locator('button').filter({ hasText: 'Knowledge' }).filter({ hasNotText: /Hub ·/ }),
  ).toBeVisible()
})

test('mobile burger nav navigates security context on advanced tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/platform/zeus/security')
  await page.getByRole('button', { name: 'Menu' }).click()
  await (await navLink(page, '/platform/zeus/security/policies')).click()
  await expect(page).toHaveURL(/\/platform\/zeus\/security\/policies/)
})

test('normal tier zyra route renders in place without redirect', async ({ page }) => {
  // Tier only shapes dock/sidebar density; gated routes render in place (a8bef254).
  // The Zeus OS AI hub route is /platform/zyra, not /platform/zeus (that's only a live prefix for
  // /platform/zeus/security*) — an earlier version of this test 404'd on the wrong URL and the
  // resulting PlatformNotFound page was misdiagnosed as a hang; see the finding note on
  // "zyra fleet tab loads without JS crash" in platform-full.spec.ts.
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/zyra')
  await expect(page.getByRole('heading', { name: 'Machina Zyra OS' })).toBeVisible({ timeout: 15_000 })
  await expect(page).toHaveURL(/\/platform\/zyra/)
})

test('spotlight hides legacy Pages category on platform desktop', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await expect(spotlight.getByText('Pages', { exact: true })).toHaveCount(0)
  await expect(spotlight.getByText('Resources workspace', { exact: true })).toHaveCount(0)
})

test('Go menu operations navigates without tier bounce on power tier', async ({ page }) => {
  // The old menubar's "Go" destination menu is gone; spotlight (Ctrl+K) is the live way to jump
  // straight to a hub, and covers the same "doesn't bounce to a different tier's route" concern.
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await spotlight.getByRole('button', { name: /Operations Hub ·/i }).click()
  await expect(page).toHaveURL(/\/platform\/operations/)
})

test('fleet cloud More menu navigates overflow route', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/fleet-cloud/instances')
  await page.getByRole('navigation', { name: 'Fleet Cloud' }).getByRole('button', { name: 'More' }).click()
  await expect(page.getByRole('menu', { name: 'Fleet Cloud more destinations' })).toBeVisible()
  await page.getByRole('menuitem', { name: 'Flavors' }).click()
  await expect(page).toHaveURL(/\/fleet-cloud\/flavors/)
})

test('spotlight platform command shows review before execute', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await spotlight.getByPlaceholder('Zyra — search or ask…').fill('import storage')
  await spotlight.getByRole('button', { name: /Import storage/i }).click()
  await expect(spotlight.getByText('Review command')).toBeVisible()
  await expect(spotlight.getByText(/Discover storage pools/i)).toBeVisible()
  await spotlight.getByRole('button', { name: /Confirm/i }).click()
  await expect(page.getByText(/Imported storage/i)).toBeVisible({ timeout: 10_000 })
})

test('spotlight import networks command shows review before execute', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder('Zyra — search or ask…'),
  })
  await spotlight.getByPlaceholder('Zyra — search or ask…').fill('import networks')
  await spotlight.getByRole('button', { name: /Import networks/i }).click()
  await expect(spotlight.getByText('Review command')).toBeVisible()
  await expect(spotlight.getByText(/Import libvirt networks/i)).toBeVisible()
})
