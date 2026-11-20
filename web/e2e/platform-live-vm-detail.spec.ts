// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live E2E: Platform VM detail — Cockpit parity (devices, NMI, description, pending-config).

import { test, expect } from '@playwright/test'
import {
  liveBaseUrl,
  openLiveVmDetail,
  openVmDetailTab,
  platformApiGet,
  skipUnlessLiveVm,
} from './helpers/liveVm'

test.describe.configure({ mode: 'serial' })

test.beforeEach(({ page: _page }, testInfo) => {
  skipUnlessLiveVm(testInfo)
})

test.describe('Platform VM detail (live)', () => {
  test.beforeEach(async ({ page }) => {
    await openLiveVmDetail(page)
  })

  test('devices tab shows watchdog and virtiofs controls', async ({ page }) => {
    await openVmDetailTab(page, 'Devices')
    await expect(page.getByTestId('vm-devices-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByTestId('vm-watchdog-row')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Attach watchdog' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Shared directories (virtiofs)' })).toBeVisible()
  })

  test('disks tab exposes edit and eject controls when inventory loaded', async ({ page }) => {
    await openVmDetailTab(page, 'Disks')
    await expect(page.getByTestId('vm-disks-panel')).toBeVisible({ timeout: 30_000 })
    const editBtn = page.getByRole('button', { name: 'Edit' }).first()
    if (await editBtn.isVisible().catch(() => false)) {
      await editBtn.click()
      await expect(page.getByRole('button', { name: 'Apply' })).toBeVisible()
    }
  })

  test('network tab exposes NIC edit controls', async ({ page }) => {
    await openVmDetailTab(page, 'Network')
    await expect(page.getByTestId('vm-network-panel')).toBeVisible({ timeout: 30_000 })
    const editBtn = page.getByRole('button', { name: 'Edit' }).first()
    if (await editBtn.isVisible().catch(() => false)) {
      await editBtn.click()
      await expect(page.getByRole('button', { name: 'Apply' })).toBeVisible()
    }
  })

  test('running VM exposes force reboot when power actions visible', async ({ page }) => {
    const force = page.getByTestId('vm-force-reboot-button')
    if (!(await force.isVisible().catch(() => false))) {
      test.skip(true, 'VM not running — force reboot hidden')
    }
    await expect(force).toBeEnabled()
  })

  test('devices tab lists vsock row', async ({ page }) => {
    await openVmDetailTab(page, 'Devices')
    await expect(page.getByTestId('vm-devices-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByTestId('vm-vsock-row')).toBeVisible()
    await expect(page.getByText(/vsock/i).first()).toBeVisible()
  })

  test('running VM exposes NMI power action', async ({ page }) => {
    const nmi = page.getByTestId('vm-nmi-button')
    const running = await nmi.isVisible().catch(() => false)
    if (!running) {
      test.skip(true, 'VM not running — NMI only shown for running guests')
    }
    await expect(nmi).toBeEnabled()
  })

  test('settings tab saves VM description', async ({ page }) => {
    await openVmDetailTab(page, 'Settings')
    const input = page.getByTestId('vm-description-input')
    await expect(input).toBeVisible({ timeout: 15_000 })
    const marker = `e2e-desc-${Date.now()}`
    await input.fill(marker)
    await page.getByRole('button', { name: 'Save description' }).click()
    await page.reload({ waitUntil: 'domcontentloaded' })
    await openVmDetailTab(page, 'Settings')
    await expect(page.getByTestId('vm-description-input')).toHaveValue(marker, { timeout: 30_000 })
  })

  test('pending-config API reachable when VM running', async ({ page }) => {
    const vmId = process.env.PLAYWRIGHT_LIBVIRT_VM_ID!.trim()
    const res = await platformApiGet(page, `/api/v1/vms/${vmId}/pending-config`)
    expect(res.status()).toBeLessThan(500)
    if (res.status() === 200) {
      const body = (await res.json()) as { needs_shutdown?: boolean; pending_changes?: unknown[] }
      expect(body).toHaveProperty('needs_shutdown')
      expect(Array.isArray(body.pending_changes)).toBe(true)
    }
  })

  test('qemu logs tab loads for libvirt VM', async ({ page }) => {
    await openVmDetailTab(page, 'Logs')
    await expect(page.getByRole('heading', { name: 'QEMU log' })).toBeVisible({ timeout: 45_000 })
    const vmId = process.env.PLAYWRIGHT_LIBVIRT_VM_ID!.trim()
    const res = await platformApiGet(page, `/api/v1/vms/${vmId}/qemu-logs?lines=50`)
    expect(res.status()).toBeLessThan(500)
  })
})
