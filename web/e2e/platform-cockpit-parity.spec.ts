// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('Cockpit parity surfaces (mock)', () => {
  test.describe.configure({ retries: 1 })

  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
  })

  test('VM overview compute panel opens CPU topology modal', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('heading', { name: 'Compute' })).toBeVisible()
    await expect(page.getByText('1×2×1')).toBeVisible({ timeout: 10_000 })
    await page.getByRole('button', { name: 'Edit CPU' }).click()
    await expect(page.getByText(/CPU topology — vm-1/i)).toBeVisible()
    await expect(page.getByText('Active vCPUs:')).toBeVisible()
  })

  test('VM overview shows hypervisor resources panel', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('heading', { name: 'Live usage' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Compute' })).toBeVisible()
  })

  test('Storage pool volumes expand and list', async ({ page }) => {
    await page.goto('/platform/storage')
    await page.getByRole('tab', { name: 'Pools' }).click()
    const row = page.getByTestId('storage-pool-default')
    await expect(row).toBeVisible({ timeout: 15_000 })
    // The toggle button has no testid and reads "Volumes" / "Hide vols", not "Hide volumes".
    const toggle = row.getByRole('button', { name: /volumes|vols/i })
    if (!(await toggle.textContent())?.toLowerCase().includes('hide')) {
      await toggle.click()
    }
    await expect(page.getByText('vol-a')).toBeVisible()
  })

  test('Machine Finder shows guest IP from batch fallback', async ({ page }) => {
    await page.goto('/platform/vms?lens=table')
    await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('192.168.122.50').first()).toBeVisible({ timeout: 15_000 })
  })

  test('Snapshots panel loads with precheck API', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    // Scoped to the detail-page tab strip: GlobalBar now also has its own "More" nav-group button.
    await page.locator('#platform-detail-tabs').getByRole('button', { name: 'More' }).click()
    await page.getByRole('menuitem', { name: 'Snapshots', exact: true }).click()
    await expect(page.getByTestId('vm-snapshots-panel')).toBeVisible({ timeout: 15_000 })
  })

  test('VM overview shows live CPU and memory usage bars', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    await expect(page.getByTestId('vm-usage-bars')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('vm-usage-cpu')).toBeVisible()
    await expect(page.getByTestId('vm-usage-memory')).toBeVisible()
  })

  test('Machine Finder resource strip shows pool and network counts', async ({ page }) => {
    await page.goto('/platform/vms?lens=table')
    const strip = page.getByTestId('machine-finder-resource-strip')
    await expect(strip).toBeVisible({ timeout: 15_000 })
    await expect(strip.getByText(/active/i).first()).toBeVisible()
  })

  test('Host detail storage tab shows Cockpit storaged inventory', async ({ page }) => {
    await page.goto('/platform/hosts/h1?tab=storage')
    await expect(page.getByTestId('host-cockpit-storage')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('RAID (mdadm)')).toBeVisible()
    await expect(page.getByText('/dev/md0')).toBeVisible()
  })

  test('Host detail system tab shows kdump and SELinux panels', async ({ page }) => {
    await page.goto('/platform/hosts/h1?tab=system')
    await expect(page.getByTestId('host-cockpit-system')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByRole('button', { name: 'Enforcing' })).toBeVisible()
  })

  test('Host detail terminal tab shows embedded SSH panel', async ({ page }) => {
    await page.goto('/platform/hosts/h1?tab=terminal')
    await expect(page.getByTestId('platform-host-terminal')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText(/127\.0\.0\.1|host-1/i).first()).toBeVisible()
  })

  test('Host detail network tab shows OVS SDN and NM create wizard', async ({ page }) => {
    await page.goto('/platform/hosts/h1?tab=network')
    await expect(page.getByTestId('host-ovs-panel')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('br-int')).toBeVisible()
    await expect(page.getByTestId('host-nm-create-wizard')).toBeVisible()
    await page.getByRole('button', { name: 'vlan', exact: true }).click()
    await expect(page.getByRole('button', { name: 'Create VLAN' })).toBeVisible()
  })

  test('Host detail linux tab shows PackageKit panel', async ({ page }) => {
    await page.goto('/platform/hosts/h1?tab=linux')
    await expect(page.getByTestId('host-packagekit-panel')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('PackageKit daemon active')).toBeVisible()
  })

  test('VM settings shows storage live-migration fields', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    await page.locator('#platform-detail-tabs').getByRole('button', { name: 'More' }).click()
    await page.getByRole('menuitem', { name: 'Settings', exact: true }).click()
    await expect(page.getByTestId('vm-migrate-panel')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('Copy disk storage (non-shared)')).toBeVisible()
    await expect(page.getByPlaceholder('/var/lib/libvirt/images/vm.qcow2')).toBeVisible()
    await expect(page.getByPlaceholder('qemu+ssh://dest/system')).toBeVisible()
  })
})
