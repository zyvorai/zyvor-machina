// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live UX: create VM, delete from Machine Finder command center, capture screenshots.

import fs from 'node:fs'
import path from 'node:path'
import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')

test.skip(!live || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')

test('live machine finder create and delete with screenshots', async ({ page }, testInfo) => {
  test.setTimeout(300_000)

  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  const shotDir = path.join(testInfo.project.outputDir, 'machine-finder-delete')
  fs.mkdirSync(shotDir, { recursive: true })
  const shot = async (name: string) => {
    await page.screenshot({ path: path.join(shotDir, `${name}.png`), fullPage: true })
  }

  await setDesktopTier(page, 'power')
  await ensureLoggedIn(page, live!, '/platform')

  await page.goto(`${live}/platform/vms`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByRole('heading', { name: 'Machine Finder', exact: true })).toBeVisible({
    timeout: 45_000,
  })

  // Prefer an already-existing disposable VM; only create if one isn't present
  let vmName = ''
  let vmCard = page.locator('[data-testid^="machine-card-"]').filter({ hasText: /ux-(e2e|screenshot)-/i }).first()
  if (await vmCard.isVisible({ timeout: 5_000 }).catch(() => false)) {
    vmName = (await vmCard.locator('p').first().textContent()) ?? ''
    await shot('01-existing-vm-found')
  } else {
    // Verify Ubuntu 24.04 template is available before attempting create
    const ubuntuAvailable = await page.getByRole('button', { name: /Ubuntu 24\.04/i }).first()
      .isVisible({ timeout: 5_000 }).catch(() => false)
    if (!ubuntuAvailable) {
      // Check in the create wizard
      vmName = `ux-screenshot-${Date.now()}`
      await page.goto(`${live}/platform/vms?create=${vmName}`, { waitUntil: 'domcontentloaded' })
      const ubuntuBtn = page.getByRole('button', { name: /Ubuntu 24\.04/i }).first()
      if (!await ubuntuBtn.isVisible({ timeout: 20_000 }).catch(() => false)) {
        testInfo.skip(true, 'Ubuntu 24.04 template not available on this host')
        return
      }
      await page.locator('input.input').first().fill(vmName)
      await page.getByRole('button', { name: 'Next' }).click()
      await ubuntuBtn.click()
      await page.getByRole('button', { name: 'Next' }).click()
      await page.getByRole('button', { name: 'Next' }).click()
      const createBtn = page.locator('button.btn-primary.min-w-\\[7rem\\]').filter({ hasText: /^Create VM$/ })
      await expect(createBtn).toBeEnabled({ timeout: 30_000 })
      const createResp = page.waitForResponse(
        (r) => r.url().includes('/platform/controller/api/v1/vms') && r.request().method() === 'POST' && r.status() < 500,
        { timeout: 120_000 },
      )
      await createBtn.click()
      expect((await createResp).status()).toBeLessThan(500)
      await page.goto(`${live}/platform/vms`, { waitUntil: 'domcontentloaded' })
      await expect(page.getByRole('heading', { name: 'Machine Finder', exact: true })).toBeVisible({ timeout: 45_000 })
      await expect(page.getByText(vmName, { exact: false }).first()).toBeVisible({ timeout: 90_000 })
    }
    vmCard = page.locator('[data-testid^="machine-card-"]').filter({ hasText: vmName }).first()
    await shot('01-create-wizard')
  }

  await expect(page.getByText(vmName, { exact: false }).first()).toBeVisible({ timeout: 90_000 })
  await shot('02-machine-finder-with-vm')

  vmCard = page.locator('[data-testid^="machine-card-"]').filter({ hasText: vmName }).first()
  await expect(vmCard).toBeVisible({ timeout: 30_000 })
  const testId = await vmCard.getAttribute('data-testid')
  const vmId = testId?.replace('machine-card-', '') ?? ''
  expect(vmId.length).toBeGreaterThan(8)

  await vmCard.click()
  await expect(page.getByTestId('machine-finder-command-center')).toBeVisible({ timeout: 15_000 })
  await shot('03-command-center-before-delete')

  page.once('dialog', (d) => d.accept())
  const deleteReq = page.waitForResponse(
    (r) => r.url().includes(`/vms/${vmId}/delete`) && r.request().method() === 'POST',
    { timeout: 90_000 },
  )
  await page.getByTestId('machine-finder-command-center').getByRole('button', { name: 'Delete' }).click()
  const deleteRes = await deleteReq
  expect(deleteRes.status()).toBeLessThan(500)

  await expect(page.locator(`[data-testid="machine-card-${vmId}"]`)).toHaveCount(0, { timeout: 60_000 })
  await shot('04-machine-finder-after-delete')

  await expect(page.getByText(/Application error|Something went wrong|Machina daemon is not responding/i)).toHaveCount(
    0,
  )
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])

  const health = await page.request.get(`${live}/api/v1/health`, { ignoreHTTPSErrors: true })
  expect(health.ok()).toBeTruthy()
  const healthBody = (await health.json()) as { status?: string }
  expect(healthBody.status).toBe('healthy')

  await shot('05-final-healthy-state')
})
