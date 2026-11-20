// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live UX: Create VM wizard on a deployed host (requires PAM credentials).

import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')

test.skip(!live || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')

test.describe.configure({ mode: 'serial' })

test.beforeEach(async ({ page }) => {
  await setDesktopTier(page, 'power')
  await ensureLoggedIn(page, live!, '/platform')
})

test('live create VM wizard completes without page crash', async ({ page }) => {
  test.setTimeout(180_000)
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  const vmName = `ux-e2e-${Date.now()}`
  await page.goto(`${live}/platform/vms?create=${vmName}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({
    timeout: 30_000,
  })

  await page.locator('input.input').first().fill(vmName)
  await page.getByRole('button', { name: 'Next' }).click()

  // Pick Ubuntu 24.04 LTS (golden image likely cached on host)
  await page.getByRole('button', { name: /Ubuntu 24\.04/i }).first().click()
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByRole('button', { name: 'Next' }).click()

  await expect(page.getByText('SSH public key (optional)')).toBeVisible({ timeout: 10_000 })
  const createBtn = page.locator('button.btn-primary.min-w-\\[7rem\\]').filter({ hasText: /^Create VM$/ })
  await expect(createBtn).toBeEnabled({ timeout: 30_000 })

  const createResp = page.waitForResponse(
    (r) =>
      r.url().includes('/platform/controller/api/v1/vms') &&
      (r.request().method() === 'POST') &&
      r.status() < 500,
    { timeout: 120_000 },
  )
  await createBtn.click()
  const res = await createResp
  expect(res.status()).toBeLessThan(500)

  await page.waitForTimeout(2000)
  await expect(page.getByText(/Application error|Something went wrong/i)).toHaveCount(0)
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
})

test('live marketplace shows current OS templates', async ({ page }) => {
  test.setTimeout(120_000)
  await page.goto(`${live}/platform/vms?create=marketplace-check`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({ timeout: 30_000 })
  await page.getByRole('button', { name: 'Next' }).click()
  await expect(page.getByRole('button', { name: /Fedora 44/i }).first()).toBeVisible({ timeout: 20_000 })
  await expect(page.getByRole('button', { name: /Ubuntu 25\.10/i }).first()).toBeVisible()
  await expect(page.getByRole('button', { name: /Debian 13/i }).first()).toBeVisible()
  await expect(page.getByRole('button', { name: /^Fedora 40\b/i })).toHaveCount(0)
})
