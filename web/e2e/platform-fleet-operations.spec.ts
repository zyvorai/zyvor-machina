// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('threat hunting structured SIEM search returns index hits', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/hunt')
  await expect(page.getByRole('heading', { name: 'Threat hunting' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Search index' }).click()
  await expect(page.getByText('nc listener on port 4444')).toBeVisible({ timeout: 10_000 })
})

test('maintenance updates tab shows agent upgrade matrix', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/maintenance?tab=updates')
  await expect(page.getByText('Agent upgrade matrix')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Agents below minimum lose guest health and enforcement RPCs.')).toBeVisible()
})

test('topology digital twin runs single-target impact analyze', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/topology')
  await expect(page.getByText('Infrastructure Digital Twin')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('twin-impact-analyze').click()
  await expect(page.getByText('1 VM would lose compute if host-1 shuts down')).toBeVisible({ timeout: 10_000 })
})
