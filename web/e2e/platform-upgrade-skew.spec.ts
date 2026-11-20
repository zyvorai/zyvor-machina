// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('Upgrade skew', () => {
  test('shows each host\'s agent version status, the safe order and what blocks an upgrade', async ({ page }) => {
    await mockPlatformApi(page)
    const host = (id: string, hostname: string, agent_version: string, status: string, reason: string, running_vms: number, blocker: string | null) =>
      ({ id, hostname, agent_version, status, reason, running_vms, maintenance_mode: false, blocker })
    await page.route('**/api/v1/upgrade/matrix', (r) =>
      r.fulfill({ json: {
        controller_version: '0.2.0', recommended_agent: '0.2.0', min_agent: '0.1.0', notes: 'n',
        order: ['controller', 'kvm-b', 'kvm-a', 'kvm-c'],
        hosts: [
          host('1', 'kvm-a', '0.2.0', 'current', 'Matches the controller.', 3, 'This host runs 3 machine(s). Put it into maintenance first.'),
          host('2', 'kvm-b', '0.1.9', 'supported', 'One minor version behind.', 0, null),
          host('3', 'kvm-c', '', 'unknown', 'The agent has not reported a version yet.', 1, 'This host runs 1 machine(s). Put it into maintenance first.'),
        ],
      } }))
    await page.goto('/platform/maintenance?tab=updates')
    const panel = page.getByTestId('upgrade-skew')
    await expect(panel).toBeVisible({ timeout: 15_000 })
    await expect(panel).toContainText('controller → kvm-b → kvm-a → kvm-c')
    await expect(panel.locator('[data-skew="current"]')).toContainText('Up to date')
    await expect(panel.locator('[data-skew="supported"]')).toContainText('One version behind')
    await expect(panel.locator('[data-skew="unknown"]')).toContainText('version unknown')
    await expect(panel.locator('[data-skew="current"]')).toContainText('Put it into maintenance first')
  })

  test('an older controller without per-host data still renders the panel', async ({ page }) => {
    await mockPlatformApi(page)
    await page.route('**/api/v1/upgrade/matrix', (r) =>
      r.fulfill({ json: { controller_version: '0.1.0', recommended_agent: '0.1.0', min_agent: '0.1.0', notes: 'n' } }))
    await page.goto('/platform/maintenance?tab=updates')
    await expect(page.getByText('Agent upgrade matrix')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('upgrade-skew')).toHaveCount(0)
  })
})
