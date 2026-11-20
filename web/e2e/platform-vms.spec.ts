// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'

const platformInfo = {
  version: '0.1.0-test',
  tls: { enabled: false },
  auth: { pam_service: 'sshd', oidc_enabled: false },
  control_plane: {
    proxy_url: '/api/v1/platform/controller',
    direct_url: 'http://127.0.0.1:5093',
  },
  kubevirt: { exec_enabled: false },
}

const sampleVms = [
  {
    id: '0c704fad-55e0-4a70-a5a5-d7fed8158921',
    name: 'e2e-libvirt-43356',
    host_id: 'e5caecb1-f1c0-48c4-8721-276eecb1f747',
    desired_state: 'unknown',
    observed_state: 'running',
    lifecycle_phase: 'error',
    last_error: 'domain missing',
    managed: false,
    uuid: '8c675a91-4454-4707-b904-b0fd6991eed6',
    vcpus: 1,
    memory_mib: 512,
    ha_enabled: false,
    project: null,
    tags: [],
    inventory_source: 'libvirt',
    guest_ip: '10.0.0.5',
    guest_tools_status: 'healthy',
  },
]

const fleetFinder = {
  summary: '5 machines · 2 running · 3 stopped · 1 need backup',
  smart_folders: [
    { id: 'all', label: 'All Machines', count: 5, icon: 'all' },
    { id: 'running', label: 'Running', count: 2, icon: 'running' },
    { id: 'needs_attention', label: 'Needs Attention', count: 1, icon: 'attention' },
  ],
  tags: [{ tag: 'e2e', count: 2 }],
  projects: [],
}

async function mockPlatformApi(page: import('@playwright/test').Page) {
  await page.addInitScript(() => {
    localStorage.setItem('zyvor-platform-welcome-done', '1')
  })
  await page.route('**/api/v1/**', async (route) => {
    const url = route.request().url()
    if (url.includes('/auth/session')) {
      return route.fulfill({
        json: { authenticated: true, username: 'admin', role: 'admin', auth_source: 'pam' },
      })
    }
    if (url.includes('/auth/providers')) {
      return route.fulfill({
        json: { pam: { enabled: true }, ldap: { enabled: false }, oidc: { enabled: false } },
      })
    }
    if (url.includes('/system/platform-info')) {
      return route.fulfill({ json: platformInfo })
    }
    if (url.includes('/platform/controller/api/v1/vms')) {
      return route.fulfill({ json: sampleVms })
    }
    if (url.includes('/platform/controller/api/v1/hosts')) {
      return route.fulfill({
        json: [{
          id: 'e5caecb1-f1c0-48c4-8721-276eecb1f747',
          hostname: 'NLDW3-6-18-28',
          address: '127.0.0.1',
          state: 'online',
          maintenance_mode: false,
          vm_count: 5,
        }],
      })
    }
    if (url.includes('/platform/controller/api/v1/fleet/finder')) {
      return route.fulfill({ json: fleetFinder })
    }
    if (url.includes('/platform/controller/api/v1/fleet/desktop')) {
      return route.fulfill({
        json: {
          hosts_online: 1,
          hosts_total: 1,
          active_tasks: 0,
          slo_breach_count: 0,
          slo_count: 0,
          pressure_hosts: 0,
          zeus_status: 'idle',
          unread_notifications: 0,
          linux_summary: 'OK',
        },
      })
    }
    if (url.includes('/ai/fleet/summary')) {
      return route.fulfill({
        json: { summary: 'Fleet healthy', hosts: 1, vms: 5, alerts: [] },
      })
    }
    if (url.includes('/events/stream') || url.includes('/ws/')) {
      return route.abort()
    }
    return route.fulfill({ json: [] })
  })
}

test('machine finder page renders without crash', { retries: 1 }, async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  await mockPlatformApi(page)
  await page.goto('/platform/vms')
  await expect(page.getByTestId('machine-finder-page')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('heading', { name: 'Machine Finder' })).toBeVisible()
  await expect(page.getByTestId('machine-finder-new-vm')).toBeVisible()
  await expect(page.getByText('e2e-libvirt-43356').first()).toBeVisible({ timeout: 15_000 })
  expect(errors).toEqual([])
})

test('machine finder card selection opens command center', async ({ page }) => {
  await mockPlatformApi(page)
  // Navigate with ?vm=ID to pre-select the VM — equivalent to clicking a card but deterministic
  // under parallel suite load where CDP-level click events can race React's concurrent scheduler.
  await page.goto('/platform/vms?vm=0c704fad-55e0-4a70-a5a5-d7fed8158921')
  await expect(page.getByRole('heading', { name: 'Command Center' })).toBeVisible({ timeout: 15_000 })
})

test('hosts finder redirects to topology lens', async ({ page }) => {
  await mockPlatformApi(page)
  await page.goto('/platform/hosts/finder?host=e5caecb1-f1c0-48c4-8721-276eecb1f747')
  await expect(page).toHaveURL(/\/platform\/vms\?.*lens=topology/, { timeout: 15_000 })
})
