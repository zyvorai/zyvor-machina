// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Machine Finder shows Infrastructure Earth globe site legend', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/vms?lens=topology')
  await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 15_000 })
  const legend = page.getByTestId('infrastructure-earth-legend')
  await expect(legend).toBeVisible({ timeout: 15_000 })
  await expect(legend.getByRole('link', { name: /DC-1/ })).toBeVisible()
  await expect(page.getByTestId('infrastructure-earth-globe')).toBeVisible()
  await expect(page.getByText(/WebGL globe|Canvas globe/)).toBeVisible()
})

test('Enterprise security strip on advanced dashboard', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform')
  const strip = page.getByTestId('enterprise-security-strip')
  await expect(strip).toBeVisible({ timeout: 15_000 })
  await expect(strip.getByText('Enterprise security', { exact: true })).toBeVisible()
  await expect(strip.getByRole('link', { name: 'Open Keychain →' })).toBeVisible()
})
