// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const VM_LIST_ROUTE = '**/platform/controller/api/v1/vms**'

test('VM list shows KubeVirt source filter', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.route(VM_LIST_ROUTE, async (route) => {
    const url = route.request().url()
    if (url.includes('source=kubevirt')) {
      return route.fulfill({
        json: [{
          id: 'kv-1',
          name: 'guest-1',
          host_id: null,
          desired_state: 'unknown',
          observed_state: 'running',
          lifecycle_phase: 'idle',
          last_error: '',
          managed: false,
          uuid: null,
          vcpus: 2,
          memory_mib: 4096,
          ha_enabled: false,
          project: null,
          tags: [],
          inventory_source: 'kubevirt',
          k8s_namespace: 'default',
          last_seen_at: new Date().toISOString(),
        }],
      })
    }
    return route.continue()
  })
  await page.goto('/platform/vms?lens=table&source=kubevirt')
  await expect(page.getByRole('link', { name: 'guest-1' })).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('table').getByText('kubevirt', { exact: true })).toBeVisible()
})

test('VM list shows missing state badge', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.route(VM_LIST_ROUTE, async (route) => {
    if (route.request().method() !== 'GET') return route.continue()
    return route.fulfill({
      json: [{
        id: 'vm-missing',
        name: 'ghost-vm',
        host_id: 'h1',
        desired_state: 'running',
        observed_state: 'missing',
        lifecycle_phase: 'idle',
        last_error: 'domain_not_found',
        managed: true,
        uuid: 'uuid-ghost',
        vcpus: 1,
        memory_mib: 1024,
        ha_enabled: false,
        project: null,
        tags: [],
        inventory_source: 'libvirt',
        k8s_namespace: null,
        last_seen_at: null,
      }],
    })
  })
  await page.goto('/platform/vms?lens=table&folder=missing')
  await expect(page.getByRole('link', { name: 'ghost-vm' })).toBeVisible({ timeout: 15_000 })
  await expect(page.locator('table').getByText('missing', { exact: true })).toBeVisible()
})
