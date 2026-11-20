// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { expect, type Page } from '@playwright/test'

/** Invisible full-screen layers that swallow clicks after partial dismiss. */
export async function assertNoShellClickBlockers(page: Page) {
  await expect(page.locator('.fixed.inset-0.z-40[aria-hidden="true"]')).toHaveCount(0)
  await expect(page.getByRole('dialog', { name: 'Mission Control' })).toHaveCount(0)
  await expect(page.locator('[aria-labelledby="dock-editor-title"]')).toHaveCount(0)
}

/**
 * The shell has one navigation surface: the top bar (`nav[aria-label="Primary"]`, a row of buttons
 * that each open a `.gnb-flyout` of items) and, at <= 1024px, the burger-opened `.gnb-sheet`. The
 * sidebar and the Mac dock are gone by design (see docs/design/APPLE-UX-CONTRACT.md).
 */
const MOBILE_MAX = 1024

function isMobile(page: Page) {
  return (page.viewportSize()?.width ?? 1280) <= MOBILE_MAX
}

export async function assertShellNavResponsive(page: Page) {
  await assertNoShellClickBlockers(page)
  const groupButton = page.getByRole('navigation', { name: 'Primary' }).getByRole('button').first()
  await groupButton.click()
  const flyoutLink = page.locator('.gnb-flyout-link').first()
  await expect(flyoutLink).toBeVisible()
  await flyoutLink.click()
  await expect(page).toHaveURL(/\/platform/)
  await assertNoShellClickBlockers(page)
}

/** Locator for a navigation item by route, opening whichever flyout / sheet section holds it. */
export async function navLink(page: Page, href: string) {
  if (isMobile(page)) {
    const sheet = page.getByRole('dialog', { name: 'Navigation' })
    if (!(await sheet.isVisible().catch(() => false))) await page.getByRole('button', { name: 'Menu' }).click()
    const link = sheet.locator(`a[href="${href}"]`).first()
    const groups = sheet.locator('details.gnb-sheet-group:not([open]) > summary')
    for (let i = 0, n = await groups.count(); i < n && !(await link.isVisible().catch(() => false)); i++) {
      await groups.first().click()
    }
    return link
  }
  const item = page.locator(`.gnb-flyout-link[data-to="${href}"]`)
  const triggers = page.getByRole('navigation', { name: 'Primary' }).locator('button.gnb-nav-item')
  await triggers.first().waitFor({ state: 'visible', timeout: 20_000 })
  for (let i = 0, n = await triggers.count(); i < n; i++) {
    await triggers.nth(i).hover()
    await page.locator('.gnb-flyout').first().waitFor({ state: 'visible', timeout: 2000 }).catch(() => {})
    if (await item.isVisible().catch(() => false)) return item
  }
  return item
}

/** Click a random item from a random top-bar flyout and confirm the route changed. */
export async function clickRandomNavLink(page: Page) {
  const triggers = page.getByRole('navigation', { name: 'Primary' }).locator('button.gnb-nav-item')
  await triggers.first().waitFor({ state: 'visible', timeout: 20_000 })
  const n = await triggers.count()
  await triggers.nth(Math.floor(Math.random() * n)).hover()
  const items = page.locator('.gnb-flyout-link')
  await expect(items.first()).toBeVisible()
  const count = await items.count()
  const idx = Math.floor(Math.random() * count)
  const to = await items.nth(idx).getAttribute('data-to')
  await items.nth(idx).click()
  if (to) await expect(page).toHaveURL(new RegExp(`${to.split('?')[0].replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`))
}
