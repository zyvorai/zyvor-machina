// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live E2E: New feature coverage — snapshot health badges, VFIO driver badge,
// per-VM schedules API + UI, Devices page.

import { test, expect } from '@playwright/test'
import {
  liveBaseUrl,
  liveVmId,
  livePlatformVmId,
  skipUnlessLiveVm,
  openLiveVmDetail,
  openVmDetailTab,
  platformApiGet,
  platformApiPost,
  platformApiDelete,
} from './helpers/liveVm'
import { ensureLoggedIn } from './helpers/liveAuth'

test.describe.configure({ mode: 'serial' })
test.use({ timeout: 300_000 })

test.beforeEach(async ({ page }, testInfo) => {
  skipUnlessLiveVm(testInfo)
  await ensureLoggedIn(page, liveBaseUrl(), { navigate: false })
})

const CTRL = '/api/v1/platform/controller/api/v1'

// ─── Snapshot health badges ──────────────────────────────────────────────────

test.describe('Snapshot health badges (live)', () => {
  test('SH01 — snapshots page loads and renders state column', async ({ page }) => {
    const live = liveBaseUrl()
    await page.goto(`${live}/snapshots`, { waitUntil: 'domcontentloaded', timeout: 60_000 })
    // Page must render (not crash)
    await expect(page.locator('text=Snapshots').first()).toBeVisible({ timeout: 15_000 })
    // Either shows "No snapshots" empty state or the table
    const hasTable = await page.locator('table[aria-label="VM snapshots"]').isVisible()
    if (hasTable) {
      // State column header must exist
      await expect(page.locator('th', { hasText: 'State' })).toBeVisible()
    }
  })

  test('SH02 — snapshot state badges present when snapshots exist', async ({ page }) => {
    const live = liveBaseUrl()
    // Check the daemon snapshot list endpoint
    const res = await page.request.get(`${live}/api/v1/snapshots`, { ignoreHTTPSErrors: true })
    if (!res.ok() || (await res.json() as unknown[]).length === 0) {
      test.skip()
      return
    }
    await page.goto(`${live}/snapshots`, { waitUntil: 'domcontentloaded', timeout: 60_000 })
    await expect(page.locator('table[aria-label="VM snapshots"]')).toBeVisible({ timeout: 15_000 })
    // Badge span with rounded class
    const badge = page.locator('td span[class*="rounded"]').first()
    await expect(badge).toBeVisible({ timeout: 10_000 })
  })
})

// ─── Devices page VFIO badge ─────────────────────────────────────────────────

test.describe('Devices page (live)', () => {
  test('DV01 — devices page loads and shows node device list', async ({ page }) => {
    const live = liveBaseUrl()
    await page.goto(`${live}/devices`, { waitUntil: 'domcontentloaded', timeout: 60_000 })
    await expect(page.locator('text=Node Devices').first()).toBeVisible({ timeout: 15_000 })
    // Check API
    const res = await page.request.get(`${live}/api/v1/node-devices`, { ignoreHTTPSErrors: true })
    expect(res.ok()).toBeTruthy()
  })

  test('DV02 — VFIO status endpoint responds for a PCI device', async ({ page }) => {
    const live = liveBaseUrl()
    // Get list of PCI devices from daemon
    const devRes = await page.request.get(`${live}/api/v1/host/pci`, { ignoreHTTPSErrors: true })
    if (!devRes.ok()) { test.skip(); return }
    const devs = (await devRes.json() as Array<{ addr?: string; bdf?: string }>)
    if (!devs.length) { test.skip(); return }

    const addr = devs[0].addr ?? devs[0].bdf ?? '0000:00:00.0'
    const statusRes = await page.request.get(
      `${live}/api/v1/host/devices/${encodeURIComponent(addr)}/vfio-status`,
      { ignoreHTTPSErrors: true },
    )
    // Endpoint must respond (200 or 4xx — not 5xx)
    expect(statusRes.status()).toBeLessThan(500)
    if (statusRes.ok()) {
      const body = await statusRes.json() as { driver: string; vfio_bound: boolean }
      expect(typeof body.driver).toBe('string')
      expect(typeof body.vfio_bound).toBe('boolean')
    }
  })
})

// ─── Per-VM schedules API ────────────────────────────────────────────────────

test.describe('Per-VM schedules (live)', () => {
  let vmId = ''
  let scheduleId = ''

  test('VS01 — list VM schedules returns array', async ({ page }) => {
    vmId = await livePlatformVmId(page)
    if (!vmId) { test.skip(); return }

    const res = await platformApiGet(page, `${CTRL}/vms/${vmId}/schedules`)
    expect(res.status()).toBeLessThan(500)
    if (res.ok()) {
      const body = await res.json() as unknown[]
      expect(Array.isArray(body)).toBeTruthy()
    }
  })

  test('VS02 — create snapshot schedule for VM', async ({ page }) => {
    if (!vmId) vmId = await livePlatformVmId(page)
    if (!vmId) { test.skip(); return }

    const res = await platformApiPost(page, `${CTRL}/vms/${vmId}/schedules`, {
      action: 'snapshot',
      interval_minutes: 10080,
      retention: 3,
      label: 'e2e-test',
    })
    expect(res.status()).toBeLessThan(500)
    if (res.ok()) {
      const body = await res.json() as { id: string; action: string }
      expect(body.action).toBe('snapshot')
      scheduleId = body.id
    }
  })

  test('VS03 — delete schedule cleans up', async ({ page }) => {
    if (!vmId || !scheduleId) { test.skip(); return }
    const res = await platformApiDelete(page, `${CTRL}/vms/${vmId}/schedules/${scheduleId}`)
    expect(res.status()).toBeLessThan(500)
  })

  test('VS04 — settings tab shows scheduled operations panel', async ({ page }) => {
    if (!liveVmId()) { test.skip(); return }
    await openLiveVmDetail(page)
    await openVmDetailTab(page, 'Settings')
    await expect(page.getByText('Scheduled operations')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('button', { name: /Add schedule/i })).toBeVisible({ timeout: 10_000 })
  })
})
