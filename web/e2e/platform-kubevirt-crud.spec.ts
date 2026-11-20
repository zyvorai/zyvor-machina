// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('K8s Workloads KubeVirt lifecycle actions call API', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  page.on('dialog', (d) => void d.accept())
  await page.goto('/k8s/workloads')
  await expect(page.getByRole('heading', { name: /KubeVirt VirtualMachines/i })).toBeVisible({ timeout: 20_000 })

  const startReq = page.waitForRequest((req) =>
    req.url().includes('/k8s/kubevirt/virtualmachines/default/kv-vm-1/lifecycle')
    && req.method() === 'POST'
    && req.postDataJSON()?.action === 'start',
  )
  await page.getByRole('button', { name: 'Start' }).first().click()
  await startReq

  const deleteReq = page.waitForRequest((req) =>
    req.url().includes('/k8s/kubevirt/virtualmachines/default/kv-vm-1')
    && req.method() === 'DELETE',
  )
  await page.getByRole('button', { name: 'Delete' }).first().click()
  // ConfirmDialog (React modal) — click the "Delete" confirm button
  await page.getByRole('dialog').getByRole('button', { name: 'Delete' }).click()
  await deleteReq
})

test('KubeVirt create YAML panel toggles', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/k8s/workloads')
  await page.getByTestId('kubevirt-create-yaml').click()
  await expect(page.getByText('apiVersion: kubevirt.io/v1')).toBeVisible()
  await page.getByTestId('kubevirt-create-yaml').click()
  await expect(page.getByText('apiVersion: kubevirt.io/v1')).not.toBeVisible()
})
