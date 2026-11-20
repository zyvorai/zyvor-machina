// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live E2E: Platform VM operations — Cockpit-mapped disks, NICs, snapshots, migrate, hostdev, filesystems.

import { test, expect } from '@playwright/test'
import {
  liveBaseUrl,
  liveVmId,
  livePlatformVmId,
  openLiveVmDetail,
  openVmDetailTab,
  platformApiGet,
  platformApiPost,
  skipUnlessLiveVm,
  ensureVmManaged,
} from './helpers/liveVm'

const CTRL = '/api/v1/platform/controller/api/v1'

test.describe.configure({ mode: 'serial' })

test.beforeEach(({ page: _page }, testInfo) => {
  skipUnlessLiveVm(testInfo)
  test.setTimeout(480_000)
})

test.describe('Platform VM operations APIs (live)', () => {
  test('libvirt-details returns disks and interfaces', async ({ page }) => {
    await openLiveVmDetail(page)
    const platformId = await livePlatformVmId(page, liveVmId())
    const res = await platformApiGet(page, `${CTRL}/vms/${platformId}/libvirt-details`)
    expect(res.status()).toBe(200)
    const body = (await res.json()) as {
      disks?: Array<{ target: string; device: string }>
      interfaces?: Array<{ mac_address: string; source: string }>
      filesystems?: unknown[]
    }
    expect(Array.isArray(body.disks)).toBe(true)
    expect(body.disks!.length).toBeGreaterThan(0)
    expect(Array.isArray(body.interfaces)).toBe(true)
    expect(body.interfaces!.length).toBeGreaterThan(0)
    expect(Array.isArray(body.filesystems)).toBe(true)
  })

  test('platform disk records and snapshot list endpoints', async ({ page }) => {
    await openLiveVmDetail(page)
    const platformId = await livePlatformVmId(page, liveVmId())
    const [disks, snaps] = await Promise.all([
      platformApiGet(page, `${CTRL}/vms/${platformId}/disks`),
      platformApiGet(page, `${CTRL}/vms/${platformId}/snapshots`),
    ])
    expect(disks.status()).toBe(200)
    expect(snaps.status()).toBe(200)
    expect(Array.isArray(await disks.json())).toBe(true)
    expect(Array.isArray(await snaps.json())).toBe(true)
  })

  test('migrate precheck accepts request on libvirt VM', async ({ page }) => {
    await openLiveVmDetail(page)
    const platformId = await livePlatformVmId(page, liveVmId())
    const vmRes = await platformApiGet(page, `${CTRL}/vms/${platformId}`)
    expect(vmRes.status()).toBe(200)
    const vm = (await vmRes.json()) as { host_id?: string | null }
    if (!vm.host_id) {
      test.skip(true, 'VM has no host_id for migrate precheck')
    }
    const pre = await platformApiPost(page, `${CTRL}/vms/${platformId}/migrate/precheck`, {
      dest_host_id: vm.host_id,
      live: true,
    })
    expect(pre.status()).toBeLessThan(500)
    if (pre.status() === 200) {
      const body = (await pre.json()) as { checks?: unknown[] }
      expect(Array.isArray(body.checks)).toBe(true)
    }
  })

  test('host USB/PCI inventory query via libvirt proxy', async ({ page }) => {
    await openLiveVmDetail(page)
    const platformId = await livePlatformVmId(page, liveVmId())
    const vmRes = await platformApiGet(page, `${CTRL}/vms/${platformId}`)
    const vm = (await vmRes.json()) as { host_id?: string | null }
    if (!vm.host_id) {
      test.skip(true, 'VM has no host_id')
    }
    for (const action of ['host.usb', 'host.pci'] as const) {
      const res = await platformApiGet(
        page,
        `${CTRL}/hosts/${vm.host_id}/libvirt?action=${encodeURIComponent(action)}`,
      )
      expect(res.status()).toBeLessThan(500)
      if (res.status() === 200) {
        expect(await res.json()).toBeTruthy()
      }
    }
  })
})

test.describe('Platform VM operations UI (live)', () => {
  test.beforeEach(async ({ page }) => {
    await openLiveVmDetail(page)
  })

  test('disks tab shows libvirt inventory and attach form', async ({ page }) => {
    await openVmDetailTab(page, 'Disks')
    await expect(page.getByTestId('vm-disks-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByRole('heading', { name: 'Libvirt disks' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Attach disk' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Resize block device' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Attach' })).toBeVisible()
  })

  test('network tab shows NIC list and attach controls', async ({ page }) => {
    await openVmDetailTab(page, 'Network')
    await expect(page.getByTestId('vm-network-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByRole('heading', { name: 'Network interfaces' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Attach NIC' })).toBeVisible()
  })

  test('snapshots tab exposes create snapshot workflow', async ({ page }) => {
    await openVmDetailTab(page, 'Snapshots')
    await expect(page.getByTestId('vm-snapshots-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByPlaceholder('snap-01')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Create snapshot' })).toBeVisible()
  })

  test('settings tab shows migrate and clone panels', async ({ page }) => {
    await ensureVmManaged(page)
    await openVmDetailTab(page, 'Settings')
    await expect(page.getByTestId('vm-migrate-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByRole('heading', { name: /Live migrate|Migrate/i }).first()).toBeVisible()
    await expect(page.getByRole('button', { name: /Pre-check|Precheck/i }).first()).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Clone' }).first()).toBeVisible()
  })

  test('advanced tab loads hostdev passthrough controls', async ({ page }) => {
    test.setTimeout(90_000)
    await openVmDetailTab(page, 'Advanced')
    await expect(page.getByTestId('vm-advanced-panel')).toBeVisible({ timeout: 60_000 })
    await expect(page.getByTestId('vm-hostdev-panel')).toBeVisible({ timeout: 20_000 })
    await expect(page.getByText('USB devices on host')).toBeVisible()
    await expect(page.getByText('PCI devices')).toBeVisible()
  })

  test('devices tab lists virtiofs shares section', async ({ page }) => {
    await openVmDetailTab(page, 'Devices')
    await expect(page.getByTestId('vm-devices-panel')).toBeVisible({ timeout: 30_000 })
    await expect(page.getByRole('heading', { name: 'Shared directories (virtiofs)' })).toBeVisible()
  })
})

test.describe('Platform advanced create route (live)', () => {
  test('create-advanced wizard shows storage and unattended options', async ({ page }) => {
    const live = liveBaseUrl()
    await openLiveVmDetail(page)
    await page.goto(`${live}/platform/create-advanced`, { waitUntil: 'domcontentloaded' })
    // PageLayout renders h1 + PlatformStepWizard renders h2 with same title — use first()
    await expect(page.getByRole('heading', { name: 'Advanced VM install' }).first()).toBeVisible({ timeout: 30_000 })
    const nextBtn = page.getByRole('button', { name: 'Next' })
    await expect(nextBtn).toBeEnabled({ timeout: 10_000 })
    await nextBtn.click()
    await expect(page.getByText('Create new qcow2')).toBeVisible({ timeout: 10_000 })
    await expect(page.getByText('Existing disk path')).toBeVisible()
    await expect(page.getByText(/virt-install --unattended/i)).toBeVisible()
  })

  test('install API validates VM state', async ({ page }) => {
    await openLiveVmDetail(page)
    const platformId = await livePlatformVmId(page, liveVmId())
    const install = await platformApiPost(page, `${CTRL}/vms/${platformId}/install`)
    // Accept any non-500: running/managed VMs reject (400/409), stopped define-only VMs may accept (200/202).
    expect(install.status()).toBeLessThan(500)
  })

  test('devices tab hostdev attach panel when host inventory available', async ({ page }) => {
    await openLiveVmDetail(page)
    await openVmDetailTab(page, 'Devices')
    await expect(page.getByTestId('vm-devices-panel')).toBeVisible({ timeout: 30_000 })
    const hostdev = page.getByTestId('vm-hostdev-attach-panel')
    if (await hostdev.isVisible().catch(() => false)) {
      await expect(hostdev.getByText(/USB|PCI/i).first()).toBeVisible()
    }
  })

  test('create-advanced wizard renders install sources', async ({ page }) => {
    const live = liveBaseUrl()
    await openLiveVmDetail(page)
    await page.goto(`${live}/platform/create-advanced`, { waitUntil: 'domcontentloaded' })
    await expect(page.getByRole('heading', { name: 'Advanced VM install' }).first()).toBeVisible({ timeout: 30_000 })
    await expect(page.getByText('Automatic OS install')).toBeVisible()
    await expect(page.getByText('Network boot (PXE)')).toBeVisible()
    await expect(page.getByText('Define only')).toBeVisible()
  })
})
