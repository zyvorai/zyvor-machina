// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('tasks page shows controller health and task detail', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/tasks')
  await expect(page.getByText('Controller health')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('ctrl-test-1')).toBeVisible()
  await page.getByRole('button', { name: 'Details' }).first().click()
  await expect(page.getByText('Task detail')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('vm.migrate').first()).toBeVisible()
})

test('events page shows platform event bus', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/events')
  await expect(page.getByRole('heading', { name: 'Platform events' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Host h1 inventory sync completed')).toBeVisible()
})

test('host detail runs health check and validation', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/hosts/h1')
  await expect(page.getByRole('heading', { name: 'host-1' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Health check' }).click()
  await expect(page.getByText('Root FS above 85%')).toBeVisible({ timeout: 10_000 })
  await page.getByRole('button', { name: 'Validate now' }).click()
  await expect(page.getByText('Agent certificate valid')).toBeVisible({ timeout: 10_000 })
})

test('reports autopilot run shows success toast', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/reports?tab=reports')
  await expect(page.getByRole('heading', { name: 'Autopilot' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Run safe autopilot' }).click()
  await expect(page.getByText('Autopilot: 1 executed, 0 skipped')).toBeVisible({ timeout: 10_000 })
})

test('enterprise tenant policy save', { retries: 1 }, async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/enterprise?tab=tenants')
  await page.getByRole('button', { name: 'Save policy' }).click()
  await expect(page.getByText('Tenant policy saved')).toBeVisible({ timeout: 10_000 })
})
