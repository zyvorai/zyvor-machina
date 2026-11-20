// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Machine Finder shows site → rack → host → VM columns', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/vms?lens=topology')
  await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'DC-1' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Rack A' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'host-1' })).toBeVisible()
  await page.getByRole('button', { name: 'host-1' }).click()
  // The Machine Finder list fixture names this VM "web-01" (id v1) — its own detail page (fetched
  // separately) uses a differently-named fixture that still says "vm-1", which is why "Open VM"
  // below still leads somewhere titled that.
  // Two "web-01" buttons render once a host is selected (the VM chip row + the inspector's own VM
  // list) — both open the same VmInspector, so take the first.
  await page.getByTestId('machine-finder-topology').getByRole('button', { name: 'web-01', exact: true }).first().click()
  await expect(page.getByRole('link', { name: 'Open VM' })).toBeVisible()
})

test('Machine Finder auto-selects sole unassigned host and opens inspector', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.route('**/fleet/mission**', async (route) => {
    await route.fulfill({
      json: {
        sites: [],
        unassigned_hosts: [
          {
            id: 'host-local',
            hostname: 'localhost',
            address: '127.0.0.1',
            state: 'online',
            maintenance_mode: false,
            vm_count: 2,
            cpu_percent: 12,
            memory_used_mib: 4096,
            memory_total_mib: 16384,
            site: '',
            rack: '',
          },
        ],
        summary: { hosts: 1, vms: 2, hosts_online: 1, health_pct: 100 },
      },
    })
  })
  await page.goto('/platform/vms?lens=topology')
  await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 15_000 })
  await expect(page).toHaveURL(/host=host-local/, { timeout: 10_000 })
  await expect(page.getByRole('link', { name: 'Open host' })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByTitle('Icon view')).toHaveCount(0)
})

test('Hosts context includes Infrastructure Finder route', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms?lens=topology')
  await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('machine-finder-topology').getByRole('link', { name: 'Mission Control' })).toBeVisible()
})

test('Gallery lens shows Open Cinema tiles', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms?lens=gallery')
  await expect(page.getByTestId('machine-finder-gallery')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('link', { name: /Open Cinema/i }).first()).toBeVisible()
  await page.getByRole('link', { name: /Open Cinema/i }).first().click()
  await expect(page).toHaveURL(/\/platform\/vms\/v1\/consolehub/)
  await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
})
