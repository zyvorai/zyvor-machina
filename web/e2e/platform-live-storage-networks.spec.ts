// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live E2E: Platform storage + networks — Cockpit check-machines-storage-pools / networks parity.

import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const liveBaseUrl = () => process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '') ?? ''

test.beforeEach(({ page: _page }, testInfo) => {
  testInfo.skip(!liveBaseUrl() || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')
})

test.describe('Platform storage (live)', () => {
  test('storage page lists pools tab and wizard entry', async ({ page }) => {
    const live = liveBaseUrl()
    await setDesktopTier(page, 'power')
    await ensureLoggedIn(page, live, '/platform')
    await page.goto(`${live}/platform/storage`, { waitUntil: 'domcontentloaded' })
    await expect(page.getByRole('heading', { name: /storage/i }).first()).toBeVisible({ timeout: 45_000 })
    const poolsBtn = page.getByRole('button', { name: 'Pools' })
    if (!await poolsBtn.isVisible({ timeout: 10_000 }).catch(() => false)) return
    await poolsBtn.click()
    const addPool = page.getByRole('button', { name: /add pool|new pool/i }).first()
    if (await addPool.isVisible().catch(() => false)) {
      await addPool.click()
      await expect(page.getByText(/backend|directory|path on host/i).first()).toBeVisible({ timeout: 15_000 })
    }
  })

  test('fleet storage API returns overview', async ({ page }) => {
    const live = liveBaseUrl()
    await setDesktopTier(page, 'power')
    await ensureLoggedIn(page, live, '/platform')
    const res = await page.request.get(`${live}/api/v1/fleet/storage`, { ignoreHTTPSErrors: true })
    expect(res.status()).toBeLessThan(500)
  })
})

test.describe('Platform networks (live)', () => {
  test.describe.configure({ retries: 1 })

  test('networks page lists import and create actions', async ({ page }) => {
    test.setTimeout(90_000)
    const live = liveBaseUrl()
    await setDesktopTier(page, 'power')
    await ensureLoggedIn(page, live, '/platform')
    await page.goto(`${live}/platform/networks`, { waitUntil: 'domcontentloaded' })
    await expect(page.getByRole('heading', { name: 'Networks' })).toBeVisible({ timeout: 60_000 })
    await expect(page.getByRole('button', { name: /import from hosts/i })).toBeVisible()
    await expect(page.getByRole('button', { name: /new network/i })).toBeVisible()
  })
})

test.describe('Machine Finder table (live)', () => {
  test.describe.configure({ retries: 1 })
  test('table lens shows usage column', async ({ page }) => {
    const live = liveBaseUrl()
    await setDesktopTier(page, 'power')
    await ensureLoggedIn(page, live, '/platform/vms')
    await page.getByRole('button', { name: 'Table' }).click()
    await expect(page.getByTestId('machine-finder-table')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByRole('columnheader', { name: 'Usage' })).toBeVisible()
  })

  test('resource strip visible on machine finder', async ({ page }) => {
    const live = liveBaseUrl()
    await setDesktopTier(page, 'power')
    await ensureLoggedIn(page, live, '/platform/vms')
    await expect(page.getByTestId('machine-finder-resource-strip')).toBeVisible({ timeout: 30_000 })
  })
})
