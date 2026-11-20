// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('networks page patches bridge and VLAN', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/networks')
  await expect(page.getByRole('heading', { name: 'Networks' })).toBeVisible({ timeout: 15_000 })
  await page.locator('input[placeholder="virbr0"]').first().fill('br0')
  await page.locator('input[inputmode="numeric"]').first().fill('100')
  await page.getByTestId('network-save-n1').click()
  await expect(page.getByText('Updated default')).toBeVisible({ timeout: 10_000 })
})

test('templates page shows fleet catalog and webhook sync', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/templates')
  await expect(page.getByText('Fleet template catalog')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('rhel-9:1.0.0')).toBeVisible()
  await page.getByRole('button', { name: 'Webhook sync' }).click()
  await expect(page.getByText('Webhook sync: 2 template(s) (webhook)')).toBeVisible({ timeout: 10_000 })
})

test('topology digital twin runs batch simulate', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/topology')
  await expect(page.getByText('Infrastructure Digital Twin')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('twin-batch-simulate').click()
  await expect(page.getByText('Batch: host shutdown affects 1 VM').first()).toBeVisible({ timeout: 10_000 })
})

test('topology LLDP strip shows real per-host neighbor data, not synthesized graph edges', async ({ page }) => {
  // Wiring-audit fix: this strip used to fabricate every row from the topology graph's generic
  // "uplink" edges (hardcoded local_interface: 'uplink', port_id: '') instead of calling the real
  // per-host LLDP endpoint (GET /api/v1/hosts/{id}/lldp) that PlatformHostDetail already uses.
  await mockPlatformApi(page, { tier: 'power' })
  await page.route('**/hosts/h1/lldp', async (route) => {
    return route.fulfill({
      json: {
        source: 'lldpctl',
        neighbors: [{
          local_interface: 'eno1', chassis_id: 'aa:bb:cc:dd:ee:ff', system_name: 'tor-switch-42',
          port_id: 'Gi1/0/24', port_description: '', system_description: '', capabilities: '',
        }],
        raw_text: '', summary: '1 neighbor',
      },
    })
  })
  await page.goto('/platform/topology')
  await expect(page.getByText('LLDP uplink strip')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('eno1')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('tor-switch-42')).toBeVisible()
  await expect(page.getByText('(Gi1/0/24)')).toBeVisible()
})

test('firewall overview scores target and requests approval', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zeus/security/firewall')
  await expect(page.getByText('Risk scoring & approvals')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Score sample target' }).click()
  await expect(page.getByText('Score: 78')).toBeVisible({ timeout: 10_000 })
  await page.getByRole('button', { name: 'Request approval' }).click()
  await expect(page.getByText('Firewall change approval requested')).toBeVisible({ timeout: 10_000 })
})

test('developer page shows daemon health strip', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/developer')
  await expect(page.getByText('Daemon health')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Reachable')).toBeVisible()
  await expect(page.getByText('Active')).toBeVisible()
})
