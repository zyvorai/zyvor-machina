// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live UX: delete VM on deployed host (regression for post-delete navigation crash).

import { test, expect, type Page } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')

test.skip(!live || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')

test.beforeEach(async ({ page }) => {
  await setDesktopTier(page, 'power')
  await ensureLoggedIn(page, live!, '/platform')
})

async function createDisposableVm(page: Page, vmName: string) {
  await page.goto(`${live}/platform/vms?create=${vmName}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('heading', { name: 'Create Virtual Machine' })).toBeVisible({
    timeout: 30_000,
  })
  await page.locator('input.input').first().fill(vmName)
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByRole('button', { name: /Ubuntu 24\.04/i }).first().click()
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByRole('button', { name: 'Next' }).click()
  const createBtn = page.locator('button.btn-primary.min-w-\\[7rem\\]').filter({ hasText: /^Create VM$/ })
  await expect(createBtn).toBeEnabled({ timeout: 30_000 })
  const createResp = page.waitForResponse(
    (r) =>
      r.url().includes('/platform/controller/api/v1/vms') &&
      r.request().method() === 'POST' &&
      r.status() < 500,
    { timeout: 120_000 },
  )
  await createBtn.click()
  expect((await createResp).status()).toBeLessThan(500)
}

test('live delete vm returns to list without page crash', async ({ page }) => {
  test.setTimeout(300_000)
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  await page.goto(`${live}/platform/vms`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('heading', { name: 'Machine Finder', exact: true })).toBeVisible({
    timeout: 45_000,
  })

  const disposable = page.locator('[data-testid^="machine-card-"]').filter({ hasText: /ux-(e2e|screenshot)-/i })
  if ((await disposable.count()) === 0) {
    test.skip(true, 'No disposable ux-e2e VMs on this host — provision one manually or run platform-live-machine-finder-delete first')
    return
  }

  const vmCard = disposable.first()
  await expect(vmCard).toBeVisible({ timeout: 120_000 })

  const testId = await vmCard.getAttribute('data-testid')
  const vmId = testId?.replace('machine-card-', '') ?? ''
  expect(vmId.length).toBeGreaterThan(8)

  await vmCard.click()
  await expect(page.getByTestId('machine-finder-command-center')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('link', { name: 'Open VM detail' }).click()

  await expect(page.locator('button.btn-destructive').filter({ hasText: 'Delete' })).toBeVisible({
    timeout: 20_000,
  })

  page.once('dialog', (d) => d.accept())
  const deleteReq = page.waitForResponse(
    (r) => r.url().includes(`/vms/${vmId}/delete`) && r.request().method() === 'POST',
    { timeout: 60_000 },
  )
  await page.locator('button.btn-destructive').filter({ hasText: 'Delete' }).click()
  const res = await deleteReq
  expect(res.status()).toBeLessThan(500)

  await expect(page).toHaveURL(/\/platform\/vms\/?$/, { timeout: 20_000 })
  await expect(page.locator(`[data-testid="machine-card-${vmId}"]`)).toHaveCount(0, { timeout: 120_000 })
  await expect(page.getByText(/Application error|Something went wrong|Machina daemon is not responding/i)).toHaveCount(0)
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])

  const health = await page.request.get(`${live}/api/v1/health`, { ignoreHTTPSErrors: true })
  expect(health.ok()).toBeTruthy()

  await page.goto(`${live}/platform/vms/${vmId}`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByText(/Application error|Something went wrong/i)).toHaveCount(0)
})
