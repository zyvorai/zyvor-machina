// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { expect, type Page } from '@playwright/test'
import { mockPlatformApi } from '../platformMock'

export const FEATURE_IDS = [
  'F01', 'F02', 'F03', 'F04', 'F05', 'F06', 'F07', 'F08', 'F09', 'F10', 'F11', 'F12', 'F13',
  'F14', 'F15', 'F16', 'F17', 'F18',
] as const

export type FeatureId = (typeof FEATURE_IDS)[number]

export const MOCK_VM_ID = 'v1'

export async function setupMockPlatform(page: Page) {
  await mockPlatformApi(page, { tier: 'power' })
}

export async function openMockVmDetail(page: Page, tab?: string) {
  const qs = tab ? `?tab=${tab}` : ''
  await page.goto(`/platform/vms/${MOCK_VM_ID}${qs}`)
  await expect(page.locator('h1').filter({ hasText: 'vm-1' })).toBeVisible({ timeout: 15_000 })
}

export async function openAccessTab(page: Page) {
  await page.getByRole('button', { name: 'Access', exact: true }).click()
  await expect(page).toHaveURL(/tab=access/)
}

export async function openPowerOverflow(page: Page) {
  await page.getByRole('button', { name: /Power & more/i }).click()
  await expect(page.getByRole('menu')).toBeVisible()
}

/** Guest agent degraded + laptop NAT path (two attention items for chip swap). */
export async function applyGuestHealthDegradedOverride(page: Page) {
  await page.route('**/api/v1/vms/*/guest/health**', async (route) => {
    await route.fulfill({
      json: {
        vm_id: MOCK_VM_ID,
        vm_name: 'vm-1',
        agent_reachable: false,
        healthy: false,
        os_pretty_name: 'Ubuntu 24.04 LTS',
        guest_ip: '192.168.122.50',
        guest_hostname: 'vm-1',
        issues: [{ id: 'agent', severity: 'warn', message: 'Guest agent not responding' }],
        summary: 'Guest agent offline',
        install_state: 'channel_only',
        channel_attached: true,
        channel_connected: false,
        agent_ping: false,
        checks: [{ id: 'agent_ping', label: 'QEMU guest agent ping', passed: false, detail: 'Timeout' }],
      },
    })
  })
}

export async function applyPendingConfigOverride(page: Page) {
  await page.route('**/api/v1/vms/*/pending-config**', async (route) => {
    await route.fulfill({
      json: {
        needs_shutdown: true,
        pending: true,
        pending_changes: [{ category: 'cpu', summary: 'vCPU count changed to 4' }],
      },
    })
  })
}

export type LiveVmPick = { id: string; name?: string; guest_ip?: string; observed_state?: string }

/** Prefer private NAT guest IP for access-flow tests. */
export async function discoverLiveVm(page: Page, baseUrl: string): Promise<LiveVmPick | null> {
  const res = await page.request.get(`${baseUrl}/api/v1/platform/controller/api/v1/vms`, {
    ignoreHTTPSErrors: true,
  })
  if (!res.ok()) return null
  const vms = (await res.json()) as Array<LiveVmPick>
  return vms.find((v) => v.guest_ip?.startsWith('192.168.')) ?? vms[0] ?? null
}

async function waitOutOfRateLimit(page: Page) {
  for (let attempt = 0; attempt < 6; attempt += 1) {
    const rateLimit = page.getByRole('alert').filter({ hasText: /Rate limit exceeded/i })
    if (!(await rateLimit.isVisible().catch(() => false))) {
      return
    }
    const waitMs = Math.min(15_000 * (attempt + 1), 60_000)
    await page.waitForTimeout(waitMs)
    const retry = page.getByRole('button', { name: 'Retry' })
    if (await retry.isVisible().catch(() => false)) {
      await retry.click()
      await page.waitForLoadState('domcontentloaded')
    }
  }
}

export async function openLiveVmDetailById(page: Page, baseUrl: string, vmId: string, tab?: string) {
  const qs = tab ? `?tab=${tab}` : ''
  await page.goto(`${baseUrl}/platform/vms/${vmId}${qs}`, { waitUntil: 'domcontentloaded' })
  await waitOutOfRateLimit(page)
  if (tab === 'doctor') {
    await expect(page.getByRole('button', { name: /Run scan|Rescan/i }).first()).toBeVisible({ timeout: 60_000 })
    return
  }
  await expect(page.getByTestId('vm-detail-action-bar')).toBeVisible({ timeout: 60_000 })
}
