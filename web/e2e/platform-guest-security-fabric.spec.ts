// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('guest health shows live observability refresh', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=guestHealth')
  await expect(page.getByText('Live guest observability')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Refresh guest observability' }).click()
  await expect(page.getByText('Cloud-init', { exact: true })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('done', { exact: true })).toBeVisible()
})

test('snapshots quiesce shows fs-freeze polling banner', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=snapshots')
  await page.getByLabel('Guest quiesce').check()
  await expect(page.getByText('Guest filesystems not frozen')).toBeVisible({ timeout: 10_000 })
})

test('machine security enforcement tab shows host policies', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/machines/h1')
  await expect(page.getByRole('heading', { name: 'Machine security' })).toBeVisible({ timeout: 15_000 })
  // The "More" overflow button is a sibling of the tab strip's role="tablist", not a descendant of
  // it (DetailTabs.tsx) — scope to the sticky tab-strip container instead.
  await page.locator('#platform-detail-tabs').getByRole('button', { name: 'More' }).click()
  await page.getByRole('menuitem', { name: 'Enforcement' }).click()
  await expect(page.getByText('Block reverse-shell listeners').first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Fleet policies')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByRole('button', { name: 'Apply here' }).first()).toBeVisible()
})

test('runtime enforcement lists new policy kinds', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/enforcement')
  await expect(page.getByText('Block shadow file read')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Block raw socket capability')).toBeVisible()
})

test('k8s firewall is NetworkPolicy-only', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zeus/security/k8s')
  await expect(page.getByText(/NetworkPolicy \(enforced by machina-cni\)/)).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/Tetragon/i)).toHaveCount(0)
})

test('host audit tab renders recent AVC events', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/hosts/h1?tab=audit')
  await expect(page.getByText(/AVC|audit/i).first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('denied { read }')).toBeVisible({ timeout: 10_000 })
})

test('storage pool snapshot policy loads on pools tab', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/storage?tab=pools')
  await expect(page.getByRole('button', { name: 'Snapshot policy' }).first()).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Snapshot policy' }).first().click()
  await expect(page.getByText('14-day libvirt snapshot retention')).toBeVisible({ timeout: 10_000 })
})
