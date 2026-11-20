// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('reports tab exposes AI export URL links', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/reports')
  await expect(page.getByText('Machina Compliance')).toBeVisible({ timeout: 15_000 })
  // Exports are auth-aware download buttons now, not plain <a href> links —
  // click each one and assert it fetches the right export endpoint.
  const complianceReq = page.waitForRequest(/\/ai\/compliance\/export/)
  await page.getByTestId('reports-compliance-export-url').first().click()
  await complianceReq
  const costReq = page.waitForRequest(/\/ai\/cost\/export\.csv/)
  await page.getByTestId('reports-cost-export-url').click()
  await costReq
  const capacityReq = page.waitForRequest(/\/ai\/capacity\/export\.csv/)
  await page.getByTestId('reports-capacity-export-url').click()
  await capacityReq
})

test('create VM shows libvirt templates, cloud-init generate, and RHEL URL', async ({ page }) => {
  test.setTimeout(60_000)
  await mockPlatformApi(page)
  await page.goto('/create', { waitUntil: 'networkidle' })
  await expect(page.getByRole('heading', { level: 1, name: 'Create new guest VM' })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByTestId('libvirt-templates-panel')).toBeVisible({ timeout: 20_000 })
  await expect(page.getByText('debian-12-small')).toBeVisible()
  await page.getByRole('button', { name: 'Automatic OS install' }).click()
  await page.locator('#os-preset').selectOption('almalinux9')
  await page.locator('input[placeholder="offline access token"]').fill('e2e-token')
  await page.getByTestId('rhel-image-url').click()
  await expect(page.getByTestId('rhel-image-result')).toContainText('access.redhat.com', { timeout: 10_000 })
  await page.locator('#vm-name').fill('e2e-vm')
  await page.getByRole('button', { name: /4\. Cloud-init/ }).click()
  const cloudInit = page.locator('#create-vm-step-3')
  await expect(cloudInit).toBeVisible()
  await cloudInit.getByPlaceholder('ubuntu').fill('ubuntu')
  await cloudInit.getByTestId('cloud-init-generate').click()
  await expect(page.getByText('Seed ISO: /var/lib/libvirt/images/seed-e2e.iso')).toBeVisible({ timeout: 10_000 })
})

test('create VM survives empty ISO catalog', async ({ page }) => {
  await mockPlatformApi(page, { emptyIsos: true })
  await page.goto('/create', { waitUntil: 'networkidle' })
  await expect(page.getByRole('heading', { level: 1, name: 'Create new guest VM' })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByTestId('libvirt-templates-panel')).toBeVisible({ timeout: 20_000 })
})

test('create VM single-page form exposes cloud-init section', async ({ page }) => {
  await mockPlatformApi(page)
  await page.goto('/create', { waitUntil: 'networkidle' })
  await expect(page.getByTestId('create-vm-single-page')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('create-vm-single-page').click()
  await expect(page.locator('#create-vm-step-3')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByTestId('cloud-init-generate-hint')).toBeVisible()
})

test('fleet page loads prometheus scrape targets', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/fleet')
  await expect(page.getByTestId('fleet-prometheus-targets-load')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('fleet-prometheus-targets-load').click()
  await expect(page.getByTestId('fleet-prometheus-targets')).toContainText('1 scrape config', { timeout: 10_000 })
})

test('SOC playbooks reload from API', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/soc')
  await page.getByRole('button', { name: 'Playbooks' }).click({ force: true })
  await expect(page.getByRole('button', { name: /notify_on_critical/ })).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('soc-playbook-reload').click()
  await expect(page.getByText('Reloaded notify_on_critical')).toBeVisible({ timeout: 10_000 })
})

test('disk images kubevirt POST bundle rebuild', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/disk-images')
  await expect(page.getByText('ubuntu.qcow2', { exact: true }).first()).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'KubeVirt' }).click({ force: true })
  await expect(page.getByText('Upload qcow2 to Kubernetes')).toBeVisible({ timeout: 10_000 })
  await page.getByTestId('kubevirt-post-bundle').click()
  await expect(page.getByText('Bundle rebuilt via POST')).toBeVisible({ timeout: 10_000 })
})

test('platform settings saves controller config', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/settings')
  await expect(page.getByText('Controller connection')).toBeVisible({ timeout: 15_000 })
  await page.getByPlaceholder('http://127.0.0.1:5093').fill('http://127.0.0.1:5093')
  // setControllerConfig now exchanges these credentials for a JWT via
  // POST /api/v1/auth/login (mocked above) instead of persisting them
  // directly — a real username/password is required for the request body.
  await page.getByPlaceholder('Basic auth user').fill('admin')
  await page.getByPlaceholder('Basic auth password').fill('test-password')
  await page.getByTestId('controller-config-save').click()
  await expect(page.getByText('Signed in — controller connection saved')).toBeVisible({ timeout: 10_000 })
})
