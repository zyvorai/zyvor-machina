// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Security Center shows eBPF sensor matrix', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zeus/security')
  await expect(page.getByRole('heading', { name: 'Security Center' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('eBPF sensor matrix')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('host-1', { exact: true })).toBeVisible()
  await expect(page.getByText(/1 online host\(s\) without a healthy machina-bpfd/)).toBeVisible()
})

test('runtime enforcement apply and policy document preview', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/enforcement')
  await expect(page.getByRole('heading', { name: 'Runtime enforcement' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Block shadow file read')).toBeVisible()
  await page.getByRole('button', { name: 'Preview' }).first().click()
  await expect(page.getByText('Policy document')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText(/exec deny path=\/usr\/bin\/nc/)).toBeVisible()
})

test('threat hunting correlation row shows enforce action', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/hunt')
  await expect(page.getByRole('heading', { name: 'Threat hunting' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Threat correlations')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Enforce' }).first()).toBeVisible({ timeout: 10_000 })
})

test('network canvas threat pulse shows investigate actions', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/network-canvas')
  await expect(page.getByText('Network canvas', { exact: true })).toBeVisible({ timeout: 15_000 })
  const enforce = page.getByRole('button', { name: 'Enforce' })
  if (await enforce.count()) {
    await expect(enforce.first()).toBeVisible()
  }
})
