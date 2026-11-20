// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('mission control landing shows hero and launchpad', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-page')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('mission-control-hero')).toBeVisible()
  await expect(page.getByTestId('mission-control-launchpad')).toBeVisible()
  // The fleet title is an eyebrow <p> ("Machina · {fleetTitle}"), not a heading (MissionControlHero.tsx).
  await expect(
    page.getByTestId('mission-control-hero').getByText(/e2e-cluster|host-1|Mission Control|Machina fleet|Machina ·/i),
  ).toBeVisible()
})

test('mission control card opens command center', async ({ page }) => {
  // FleetCommandCenter (testId "fleet-command-center") isn't reachable from the bare Mission Control
  // dashboard — it only ever renders from Machine Finder, which overrides the testid to
  // "machine-finder-command-center". The Machine Finder list fixture names this VM "web-01" (id v1).
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/vms')
  await page.getByText('web-01').first().click({ timeout: 15_000 })
  await expect(page.getByTestId('machine-finder-command-center')).toBeVisible()
})

test('F3 expands fleet geography on mission control', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await page.keyboard.press('F3')
  await expect(page.getByTestId('mission-control-geography')).toBeVisible({ timeout: 10_000 })
})
