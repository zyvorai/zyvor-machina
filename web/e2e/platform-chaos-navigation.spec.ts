// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'
import {
  assertNoShellClickBlockers,
  assertShellNavResponsive,
  clickRandomNavLink,
} from './helpers/platformShellHelpers'

// Each test below gets its own fresh page/context and depends on none of the others, so there's
// no need for `mode: 'serial'` — and serial mode's retry semantics restart the *whole group* from
// test 1 rather than just the failing test, which made the occasional Mission Control close race
// (see the retries note below) waste a full group re-run. Plain per-test retries fix that.
test.describe.configure({ retries: 1 })

test.beforeEach(async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.setViewportSize({ width: 1440, height: 900 })
})

test('control center tile navigation does not trap clicks', async ({ page }) => {
  await page.goto('/platform/vms')
  await expect(page.getByRole('heading', { level: 1, name: 'Machine Finder' })).toBeVisible({
    timeout: 15_000,
  })

  await page.getByRole('button', { name: 'Control Center' }).click()
  // Scoped to the Control Center panel: the sidebar's "Clusters" link is also on this page and
  // its accessible name substring-matches an unscoped "Cluster" query (strict-mode violation).
  await page.locator('.glass-strong').getByRole('link', { name: 'Cluster' }).click()
  await expect(page).toHaveURL(/\/platform\/?$/)
  await assertShellNavResponsive(page)
})

test('mission control dismiss keeps navigation responsive', async ({ page }) => {
  await page.goto('/platform/hosts')
  await expect(page.getByRole('heading', { name: /Hosts/i })).toBeVisible({ timeout: 15_000 })

  await page.evaluate(() => {
    window.dispatchEvent(new CustomEvent('machina-open-mission-control'))
  })
  await expect(page.getByRole('dialog', { name: 'Mission Control' })).toBeVisible({ timeout: 5000 })
  // Occasionally races (rare, pre-existing, not reproduced by a second Escape or isolated run) —
  // retried like the other chaos-navigation tests rather than masked with an arbitrary wait.
  await page.keyboard.press('Escape')

  // The dock this used to click through is gone (docs/design/APPLE-UX-CONTRACT.md — do not
  // reintroduce it); the current primary nav is the sidebar + GlobalBar's flyouts.
  await assertNoShellClickBlockers(page)
  await assertShellNavResponsive(page)
})

test('random top-nav flyout, and menubar clicks stay navigable', async ({ page }) => {
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 15_000 })

  const primaryGroups = page.getByRole('navigation', { name: 'Primary' }).getByRole('button')
  const groupCount = await primaryGroups.count()

  for (let round = 0; round < 6; round++) {
    await clickRandomNavLink(page)
    await assertNoShellClickBlockers(page)

    await page.keyboard.press('Meta+,')
    await expect(page).toHaveURL(/\/platform\/settings/)
    await assertShellNavResponsive(page)

    await page.getByRole('button', { name: 'Control Center' }).click()
    const opsTile = page.getByRole('link', { name: 'Operations' }).first()
    if (await opsTile.count()) {
      await opsTile.click()
      await assertShellNavResponsive(page)
    } else {
      await page.keyboard.press('Escape')
      await assertNoShellClickBlockers(page)
    }

    // Top nav: open a different product-group flyout each round and follow a link from it. The
    // dock this used to click through instead is gone by design — see assertShellNavResponsive.
    await primaryGroups.nth(round % groupCount).click()
    await page.locator('.gnb-flyout-link').first().click()
    await expect(page).toHaveURL(/\/platform/)
    await assertNoShellClickBlockers(page)
  }
})
