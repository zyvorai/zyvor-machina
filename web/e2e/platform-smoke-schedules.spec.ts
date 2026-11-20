// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Offline smoke test for the VM schedules UI — runs against the local preview
// server with mockPlatformApi; no live server or credentials required.

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('VM scheduled operations UI (offline)', () => {
  test('settings tab shows Scheduled operations panel', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'normal' })
    await page.goto('/platform/vms/v1?tab=settings')
    await expect(page.getByRole('heading', { name: 'Scheduled operations' })).toBeVisible({ timeout: 15_000 })
  })

  test('Add schedule button renders in settings tab', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'normal' })
    await page.goto('/platform/vms/v1?tab=settings')
    await expect(page.getByRole('button', { name: 'Add schedule' })).toBeVisible({ timeout: 15_000 })
  })

  test('interval dropdown has expected options', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'normal' })
    await page.goto('/platform/vms/v1?tab=settings')
    const intervalSelect = page.getByRole('combobox', { name: 'Interval' })
    await expect(intervalSelect).toBeVisible({ timeout: 15_000 })
    await expect(intervalSelect.getByRole('option', { name: 'Every hour' })).toBeAttached()
    await expect(intervalSelect.getByRole('option', { name: 'Every 6 hours' })).toBeAttached()
    await expect(intervalSelect.getByRole('option', { name: 'Daily' })).toBeAttached()
    await expect(intervalSelect.getByRole('option', { name: 'Weekly' })).toBeAttached()
  })

  test('action dropdown has all four schedule types', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'normal' })
    await page.goto('/platform/vms/v1?tab=settings')
    const actionSelect = page.getByRole('combobox', { name: 'Action' })
    await expect(actionSelect).toBeVisible({ timeout: 15_000 })
    await expect(actionSelect.getByRole('option', { name: 'Start' })).toBeAttached()
    await expect(actionSelect.getByRole('option', { name: 'Graceful shutdown' })).toBeAttached()
    await expect(actionSelect.getByRole('option', { name: 'Force stop' })).toBeAttached()
    await expect(actionSelect.getByRole('option', { name: 'Snapshot' })).toBeAttached()
  })
})
