// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//
// Covers the wiring-audit fixes: alert-rule / scheduled-job enable-disable (previously no PATCH
// route existed at all — see controller/src/api/{alerts,scheduled_jobs}.rs), the policy-quota
// form no longer silently overwriting vCPU/memory/storage with a fixed multiple of Max VMs, and
// the Fleet Cloud image detail/delete calls that used to 404/405 (see api/nativeTemplates.ts —
// there's no single-id lookup route, only get/delete by name+version).

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('alert rule can be disabled and re-enabled', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/alert-rules')
  await expect(page.getByRole('heading', { name: 'Alert rules' })).toBeVisible({ timeout: 15_000 })
  // Scoped to the page container: an unscoped getByText('high-cpu') also matches the toast text
  // ("Disabled/Enabled 'high-cpu'") this test itself triggers below.
  const row = page.getByTestId('platform-alert-rules-page').getByText('high-cpu', { exact: true }).locator('..').locator('..')
  await expect(row.getByText(/disabled/i)).toHaveCount(0)
  await row.getByRole('button', { name: 'Disable' }).click()
  await expect(row.getByText(/disabled/i)).toBeVisible({ timeout: 10_000 })
  await row.getByRole('button', { name: 'Enable' }).click()
  await expect(row.getByText(/disabled/i)).toHaveCount(0)
})

test('scheduled job can be disabled and re-enabled', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/scheduled-jobs')
  await expect(page.getByRole('heading', { name: 'Scheduled jobs' })).toBeVisible({ timeout: 15_000 })
  const row = page.getByTestId('platform-scheduled-jobs-page').getByText('host-inventory-refresh', { exact: true }).locator('..').locator('..')
  await expect(row.getByText(/disabled/i)).toHaveCount(0)
  await row.getByRole('button', { name: 'Disable' }).click()
  await expect(row.getByText(/disabled/i)).toBeVisible({ timeout: 10_000 })
})

test('policy quota save keeps custom vCPU/memory/storage values', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.route('**/api/v1/policy/quotas', async (route) => {
    if (route.request().method() === 'GET') {
      return route.fulfill({
        json: [{ project: 'default', max_vms: 50, max_vcpu: 999, max_memory_mib: 123456, max_storage_gib: 7777 }],
      })
    }
    if (route.request().method() === 'POST') {
      const body = route.request().postDataJSON()
      // The bug: this used to always be max_vms * 4 / 8192 / 100, clobbering the real values above.
      expect(body.max_vcpu).toBe(999)
      expect(body.max_memory_mib).toBe(123456)
      expect(body.max_storage_gib).toBe(7777)
      return route.fulfill({ json: { ok: true } })
    }
    return route.continue()
  })
  await page.goto('/platform/policy')
  await expect(page.getByRole('heading', { name: 'Policy & Quotas' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByLabel('Max vCPU')).toHaveValue('999', { timeout: 10_000 })
  await page.getByRole('button', { name: 'Save quota' }).click()
})

test('Fleet Cloud image detail loads by id and delete uses name+version', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/fleet-cloud/images')
  await expect(page.getByRole('heading', { name: 'Images' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('link', { name: 'rhel-9' }).click()
  // getTemplate(id) no longer exists — the detail page finds this by id in the already-fetched
  // list, so it must show the real fleet-only image, not a "not found" state.
  await expect(page.getByText('Private fleet RHEL 9 image')).toBeVisible({ timeout: 15_000 })

  await page.goto('/fleet-cloud/images')
  const deleteReq = page.waitForRequest(
    (req) => req.method() === 'DELETE' && req.url().includes('/templates/rhel-9/1.0.0'),
  )
  await page.getByRole('row', { name: /rhel-9/ }).getByRole('button', { name: 'Delete image' }).click()
  await page.getByRole('button', { name: 'Delete', exact: true }).click()
  await deleteReq
})

test('recommendation with an unwired fix_action shows a fallback toast instead of silently no-op-ing', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.route('**/api/v1/recommendations', async (route) => {
    return route.fulfill({
      json: [{
        id: 'r1', impact: 'Medium', title: 'Rebalance overloaded host', why: 'host-3 is at 95% CPU',
        risk: 'low', action: 'Rebalance host', fix_action: 'bulk_rebalance', object_ref: { vm_ids: [] },
      }],
    })
  })
  await page.goto('/platform/recommendations')
  await expect(page.getByRole('heading', { name: 'Recommendations' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Rebalance host' }).click()
  await expect(page.getByText(/isn't automated yet/i)).toBeVisible({ timeout: 10_000 })
})

test('firewall compliance PDF export downloads via fetch through the controller proxy, not a bare cross-origin link', async ({ page }) => {
  // The old code built a plain <a href> straight at window.location.origin + the controller-only
  // path (no daemon route exists there, and no auth header rides along with a bare navigation).
  // downloadFirewallCompliancePdf now fetches it the same authenticated way every other controller
  // export already does (downloadControllerExport) and saves the response as a blob.
  await mockPlatformApi(page, { tier: 'power' })
  let requestSeen = false
  await page.route('**/zeus-firewall/compliance/*/export.pdf', async (route) => {
    requestSeen = true
    return route.fulfill({ contentType: 'application/pdf', body: Buffer.from('%PDF-1.4 mock') })
  })
  await page.goto('/platform/zeus/security/compliance')
  await expect(page.getByRole('heading', { name: /Compliance/i }).first()).toBeVisible({ timeout: 15_000 })
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Export PDF' }).click(),
  ])
  expect(requestSeen).toBe(true)
  expect(download).toBeTruthy()
})

test('Live Preview Wall is reachable from the Mission Control launchpad', async ({ page }) => {
  // /platform/mission-control/live had no inbound link anywhere in the app.
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-launchpad')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('link', { name: /Live Preview Wall/i }).click()
  await expect(page).toHaveURL(/\/platform\/mission-control\/live/)
  await expect(page.getByTestId('mission-control-live-wall')).toBeVisible({ timeout: 15_000 })
})

test('Bare metal power action shows a real success toast, and state badges/button-disabling use the real state vocabulary', async ({ page }) => {
  // BmcPowerResult was typed as {success, message} — the backend never sends either field (its
  // real shape is {dry_run, summary, new_state, ...}), so `result.success` was always falsy and
  // every power action showed a spurious error toast regardless of the real outcome. Separately,
  // the state badge/button-disabled checks compared against 'on'/'off', but the backend writes
  // 'registered'/'powered_on'/'powered_off'/'rebooting' — so Online count was always 0 and the
  // already-in-that-state button never disabled.
  await mockPlatformApi(page, { tier: 'power' })
  let state = 'registered'
  await page.route('**/api/v1/baremetal/servers', async (route) => {
    if (route.request().method() === 'GET') {
      return route.fulfill({
        json: [{
          id: 'bm1', hostname: 'metal-01', bmc_address: '192.168.10.5', bmc_type: 'redfish',
          state, cpu_cores: 32, memory_mib: 262144, firewall_profile: 'BareMetalBmc',
          firewall_enabled: true, bmc_vlan: '', pxe_vlan: '', created_at: new Date().toISOString(),
        }],
      })
    }
    return route.continue()
  })
  await page.route('**/api/v1/baremetal/servers/bm1/power', async (route) => {
    state = 'powered_on'
    return route.fulfill({
      json: {
        server_id: 'bm1', hostname: 'metal-01', action: 'on', previous_state: 'registered',
        new_state: 'powered_on', dry_run: false,
        summary: 'BMC power command applied (preview — no live IPMI/Redfish call).',
      },
    })
  })
  await page.goto('/platform/baremetal')
  await expect(page.getByRole('heading', { name: 'Bare Metal', exact: true })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('registered')).toBeVisible()
  await page.getByRole('button', { name: 'On', exact: true }).click()
  await expect(page.getByText(/BMC power command applied/i)).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('powered_on')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByRole('button', { name: 'On', exact: true })).toBeDisabled()
})

test('Firewall connectivity simulation uses host/profile pickers, shows loading state, and distinguishes never-run from zero-results', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  let resolveSim: (v: unknown) => void = () => {}
  await page.route('**/zeus-firewall/connectivity', async (route) => {
    await new Promise((resolve) => { resolveSim = resolve })
    return route.fulfill({ json: { summary: 'Simulated', allows: [], blocks: [], warnings: [] } })
  })
  await page.goto('/platform/zeus/security/connectivity')
  await expect(page.getByRole('heading', { name: 'Connectivity Matrix' })).toBeVisible({ timeout: 15_000 })
  // Free-text inputs became pickers.
  await expect(page.getByLabel('Target host')).toBeVisible()
  await expect(page.getByLabel('Firewall profile')).toBeVisible()
  await expect(page.getByText('Run simulation').first()).toBeVisible()

  await page.getByRole('button', { name: 'Simulate' }).click()
  await expect(page.getByRole('button', { name: 'Simulating…' })).toBeDisabled()
  resolveSim(null)
  // Zero results after a real run reads differently than "never run".
  await expect(page.getByText('No allowed paths in this simulation.')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('No blocked paths in this simulation.')).toBeVisible()
})

test('Host tags round-trip: save, then a reload still shows them (not blanked)', async ({ page }) => {
  // HostDetailRow never SELECTed the tags column even though PATCH wrote it — the "Placement and
  // capability tags" field always loaded empty, and looked like a failed save because load() right
  // after a successful PATCH blanked it again.
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/hosts/h1')
  await expect(page.getByRole('heading', { name: 'host-1' })).toBeVisible({ timeout: 15_000 })
  // Existing tags from the fixture load correctly.
  await expect(page.getByLabel('Placement and capability tags')).toHaveValue('gpu, edge', { timeout: 10_000 })
  await page.getByLabel('Placement and capability tags').fill('gpu, edge, new-tag')
  await page.getByRole('button', { name: 'Save tags' }).click()
  await expect(page.getByText('Tags saved')).toBeVisible({ timeout: 10_000 })
  // The bug: this used to go blank right after the toast, because load() re-fetched a response
  // with no tags field at all.
  await expect(page.getByLabel('Placement and capability tags')).toHaveValue('gpu, edge, new-tag', { timeout: 10_000 })
})

test('VM migration history shows real hostnames and dates, not "undefined → undefined · Invalid Date"', async ({ page }) => {
  // VmMigrationRecord was typed with started_at/finished_at fields the backend never sends (only
  // created_at), and the query only selected source_host_id/dest_host_id (UUIDs), not resolved
  // hostnames — every row rendered literally as "undefined → undefined · Invalid Date".
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=events')
  await expect(page.getByRole('heading', { name: 'Migration history' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('host-1 → host-2')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText(/undefined/)).toHaveCount(0)
  await expect(page.getByText(/Invalid Date/)).toHaveCount(0)
})

test('Runtime enforcement: a rejected policy (HTTP 400) shows an error toast, not "Policy created"', async ({ page }) => {
  // The controller compiles every policy through machina-bpf before storing it; an invalid
  // match comes back as 400 enforcement_rejected and nothing is created.
  await mockPlatformApi(page, { tier: 'power' })
  await page.route('**/zeus-security/enforcement/policies', async (route) => {
    if (route.request().method() === 'POST') {
      return route.fulfill({
        status: 400,
        json: {
          error: 'invalid match: tc_allow/allow_port rules require an explicit destination IP',
          error_code: 'enforcement_rejected',
        },
      })
    }
    // route.continue() would go to the real network (no backend here) rather than falling through
    // to mockPlatformApi's own handler — answer the GET/list directly instead.
    return route.fulfill({ json: { policies: [] } })
  })
  await page.goto('/platform/zyra/security/enforcement')
  await expect(page.getByRole('heading', { name: 'Runtime enforcement' })).toBeVisible({ timeout: 15_000 })
  await page.getByLabel('Policy name').fill('bad-rule')
  await page.getByLabel('Match pattern').fill('*')
  await page.getByRole('button', { name: 'Create', exact: true }).click()
  await expect(page.getByText(/invalid match/i)).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('Policy created')).toHaveCount(0)
})
