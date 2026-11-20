// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { expect, type Page } from '@playwright/test'

/** Opens the Mission Control overlay where Infrastructure DNA lives (replaces legacy fleet insights accordion). */
export async function openMissionControlOverlay(page: Page) {
  const url = page.url()
  if (url.match(/\/platform\/?(\?|#|$)/)) {
    await page.goto('/platform/vms')
    await expect(page.getByTestId('machine-finder-page')).toBeVisible({ timeout: 15_000 })
  }
  await page.evaluate(() => {
    window.dispatchEvent(new CustomEvent('machina-open-mission-control'))
  })
}

/** Fleet insights accordion was replaced by the Mission Control overlay DNA strip. */
export async function expandFleetInsights(page: Page) {
  await openMissionControlOverlay(page)
  await expect(page.getByTestId('infrastructure-dna-strip')).toBeVisible({ timeout: 15_000 })
}

/** Assert the page scrolls like a normal document (not trapped in an inner pane). */
export async function expectPageScrolls(page: Page, opts?: { viewportHeight?: number }) {
  if (opts?.viewportHeight) {
    const size = page.viewportSize() ?? { width: 1280, height: 720 }
    await page.setViewportSize({ width: size.width, height: opts.viewportHeight })
  }
  await expect
    .poll(
      async () =>
        page.evaluate(
          () => document.documentElement.scrollHeight > document.documentElement.clientHeight + 40,
        ),
      { timeout: 10_000 },
    )
    .toBe(true)
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight))
  const atBottom = await page.evaluate(
    () => window.scrollY + window.innerHeight >= document.documentElement.scrollHeight - 80,
  )
  expect(atBottom).toBe(true)
}
