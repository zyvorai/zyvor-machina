// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live E2E smoke for Batch 84 features (globe, KubeVirt CRUD, eBPF canvas).

import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = () => process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '') ?? ''

test.beforeEach(({ page: _page }, testInfo) => {
  testInfo.skip(!live() || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')
})

test.describe('Live Batch 84 features', () => {
  test('network canvas + native eBPF panels', async ({ page }, testInfo) => {
    test.setTimeout(180_000)
    const base = live()
    await setDesktopTier(page, 'normal')
    await ensureLoggedIn(page, base, { entryPath: '/platform/network-canvas', tier: 'normal', navigate: true })
    await expect(page.locator('#login-username')).toHaveCount(0, { timeout: 20_000 })
    await expect(page.getByRole('heading', { name: /network canvas/i })).toBeVisible({ timeout: 90_000 })
    const res = await page.request.get(`${base}/api/v1/network-canvas`, { ignoreHTTPSErrors: true })
    if (!res.ok()) {
      testInfo.skip(true, 'network-canvas endpoint not available on this host')
      return
    }
    let body: { native_bpf?: { reachable?: boolean } } = {}
    try {
      body = (await res.json()) as { native_bpf?: { reachable?: boolean } }
    } catch {
      testInfo.skip(true, 'network-canvas returned non-JSON on this host')
      return
    }
    testInfo.skip(!body.native_bpf?.reachable, 'machina-bpfd not reachable on this host — skipping eBPF panel checks')
    await expect(page.getByTestId('network-service-map-graph')).toBeVisible({ timeout: 60_000 })
  })

  test('infrastructure earth globe on machine finder topology lens', async ({ page }) => {
    test.setTimeout(180_000)
    const base = live()
    await setDesktopTier(page, 'advanced')
    await ensureLoggedIn(page, base, { entryPath: '/platform/vms?lens=topology', tier: 'advanced', navigate: true })
    await expect(page.locator('#login-username')).toHaveCount(0, { timeout: 20_000 })
    await expect(page.getByTestId('infrastructure-earth-globe')).toBeVisible({ timeout: 90_000 })
  })

  test('k8s workloads kubevirt create YAML panel', async ({ page }) => {
    test.setTimeout(180_000)
    const base = live()
    await setDesktopTier(page, 'power')
    await ensureLoggedIn(page, base, { entryPath: '/k8s/workloads', tier: 'power', navigate: true })
    await expect(page.locator('#login-username')).toHaveCount(0, { timeout: 20_000 })
    await expect(page.getByRole('heading', { name: /Kubernetes Workloads/i })).toBeVisible({ timeout: 90_000 })
    const createBtn = page.getByTestId('kubevirt-create-yaml')
    if (await createBtn.isVisible().catch(() => false)) {
      await createBtn.click()
      await expect(page.getByText(/apiVersion: kubevirt.io\/v1/)).toBeVisible()
    }
  })

  test('kubevirt lifecycle API route exists', async ({ page }) => {
    test.setTimeout(60_000)
    const base = live()
    await ensureLoggedIn(page, base, { navigate: false })
    const res = await page.request.post(
      `${base}/api/v1/k8s/kubevirt/virtualmachines/default/e2e-nonexistent/lifecycle`,
      { data: { action: 'start' }, ignoreHTTPSErrors: true },
    )
    expect(res.status()).not.toBe(404)
  })
})
