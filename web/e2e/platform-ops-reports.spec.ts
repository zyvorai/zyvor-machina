// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Reports runbooks tab executes catalog playbook', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/reports?tab=runbooks')
  await expect(page.getByText('Host offline recovery')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Execute' }).click()
  await expect(page.getByRole('dialog')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByRole('dialog')).toContainText('Verify host heartbeat')
  await expect(page.getByRole('dialog')).toContainText('systemctl status libvirtd')
})

test('Reports showback tab shows project rollup', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/reports?tab=showback')
  await expect(page.getByRole('heading', { name: 'Compliance showback' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/Fleet compliance grade: B\+/i)).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('cell', { name: 'default', exact: true })).toBeVisible({ timeout: 15_000 })
})

test('Operations hub links to runbooks', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/operations')
  await page.getByRole('link', { name: 'Reports' }).click()
  await expect(page).toHaveURL(/\/platform\/reports/)
  await page.getByRole('tab', { name: 'Runbooks' }).click()
  await expect(page).toHaveURL(/\/platform\/reports\?tab=runbooks/)
  await expect(page.getByText('Host offline recovery')).toBeVisible({ timeout: 15_000 })
})

test('Reports runbooks tab shows empty catalog and executions', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power', emptyRunbooks: true })
  await page.goto('/platform/reports?tab=runbooks')
  await expect(page.getByText('No runbooks')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('No runbook executions yet')).toBeVisible({ timeout: 15_000 })
})

test('Activity Monitor loads fleet summary', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/activity')
  await expect(page.getByRole('heading', { name: 'Activity Monitor' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/1 running VM\(s\) · 1 host\(s\)/)).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('link', { name: 'vm-1' })).toBeVisible({ timeout: 15_000 })
})

test('Threat hunting workspace loads saved queries', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/security/hunt')
  await expect(page.getByRole('heading', { name: 'Threat hunting' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Reverse shell listeners' })).toBeVisible({ timeout: 15_000 })
})
