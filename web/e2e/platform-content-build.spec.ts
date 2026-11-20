// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('classic backups page polls live backup status', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/backups')
  await expect(page.getByText('b1')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('backup-status-b1').click()
  await expect(page.getByText('Copying qcow2 images (42%)')).toBeVisible({ timeout: 10_000 })
})

test('disk images probes template, notes, mkosi, and direct build', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/disk-images')
  await expect(page.getByText('Build disk (virt-image-build)')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('mkosi-workspaces-panel')).toBeVisible()
  await page.getByTestId('virt-probe-template').click()
  await expect(page.getByTestId('virt-probe-result')).toContainText('in catalog')
  await page.getByTestId('virt-template-notes').click()
  await expect(page.getByTestId('virt-notes-body')).toContainText('Debian 12 stable')
  await page.locator('#vb-out').fill('/var/lib/libvirt/images/e2e-built.qcow2')
  await page.getByTestId('virt-direct-build').click()
  await expect(page.getByText('Direct build finished')).toBeVisible({ timeout: 10_000 })
})

test('firewall operator dry-runs single-host secure', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zeus/security/firewall')
  await expect(page.getByText('AI operator')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('operator-secure-h1').click()
  await expect(page.getByText('Dry-run: would apply ProductionServer profile on host-1')).toBeVisible({ timeout: 10_000 })
})

test('k8s overview shows live API nodes count', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/k8s')
  await expect(page.getByTestId('k8s-live-nodes-stat')).toBeVisible({ timeout: 20_000 })
  await expect(page.getByTestId('k8s-live-nodes-stat')).toContainText('2', { timeout: 10_000 })
})

test('k8s workloads loads kubevirt virtualmachine CRs', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/k8s/workloads')
  await expect(page.getByText('KubeVirt VirtualMachines')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('kubevirt-load-crs').click()
  await expect(page.getByTestId('kubevirt-cr-count')).toContainText('1 CR(s)')
})
