// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.beforeEach(async ({ page }) => {
  await mockPlatformApi(page)
})

test('SOC hub overview loads', async ({ page }) => {
  await page.goto('/platform/soc')
  await expect(page.getByRole('heading', { name: 'Security Operations Center' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Open alerts')).toBeVisible()
  await expect(page.getByText('Unusual port activity')).toBeVisible()
})

test('SOC alerts tab shows detail panel', async ({ page }) => {
  await page.goto('/platform/soc')
  await expect(page.getByRole('heading', { name: 'Security Operations Center' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Alerts' }).click()
  const alertBtn = page.getByRole('button', { name: /critical_anomaly/ })
  await expect(alertBtn).toBeVisible({ timeout: 10_000 })
  await alertBtn.click()
  await expect(page.getByRole('heading', { name: 'Alert detail' })).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('MITRE ATT&CK')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('T1046')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByRole('button', { name: 'Acknowledge' })).toBeVisible({ timeout: 15_000 })
})

test('SOC Splunk integration test', async ({ page }) => {
  await page.goto('/platform/soc')
  await page.getByRole('button', { name: 'Integrations' }).click()
  await expect(page.getByText('Splunk HTTP Event Collector')).toBeVisible()
  const testReq = page.waitForResponse(
    (r) => r.url().includes('/integrations/splunk/test') && r.request().method() === 'POST' && r.ok(),
  )
  await page.getByRole('button', { name: 'Test connection' }).click()
  const body = await (await testReq).json()
  expect(String(body.message ?? '')).toMatch(/Splunk HEC accepted/i)
})

test('SOC playbooks tab loads editor', async ({ page }) => {
  await page.goto('/platform/soc')
  await page.getByRole('button', { name: 'Playbooks' }).click()
  await expect(page.getByRole('button', { name: /notify_on_critical/ })).toBeVisible()
  await expect(page.getByRole('heading', { name: /Global webhook/ })).toBeVisible()
  await expect(page.getByRole('heading', { name: /Edit: notify_on_critical/ })).toBeVisible()
})
