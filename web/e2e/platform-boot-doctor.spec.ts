// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const VM = 'vm-1'
const DOCTOR = JSON.stringify({ bootability: { score: 62, blockers: [{ title: 'Missing fstab device', message: '/dev/vdb1 is not present' }] } })

test.describe('Boot Doctor', () => {
  test.beforeEach(async ({ page }) => {
    // A powered-off machine, so the flow goes straight to the disk.
    await mockPlatformApi(page, { stoppedVm: true })
    await page.route('**/api/v1/guest-repair/capabilities', (r) =>
      r.fulfill({ json: { cli_found: true, cli_path: '/usr/local/bin/guestkit', agent_binary: '/x', agent_binary_found: true } }))
  })

  test('check disk → findings → preview → repair is gated behind the confirm box', async ({ page }) => {
    const applies: string[] = []
    await page.route('**/guest-repair/diagnose', (r) =>
      r.fulfill({ json: { vm: 'x', disk: '/d', action: 'diagnose', dry_run: true, backup: false, output: DOCTOR, exit_ok: true } }))
    await page.route('**/guest-repair/apply', (r) => {
      applies.push(r.request().postData() ?? '')
      return r.fulfill({ json: { vm: 'x', disk: '/d', action: 'repair', dry_run: true, backup: false, output: '- fstab: comment out /dev/vdb1', exit_ok: true } })
    })
    await page.goto(`/platform/vms/${VM}?action=bootdoctor`)
    const card = page.getByTestId('boot-doctor')
    await expect(card).toBeVisible({ timeout: 15_000 })
    await card.getByRole('button', { name: /check disk/i }).click()
    await expect(card.getByText('boot score 62/100')).toBeVisible()
    await expect(card.getByText(/Missing fstab device/)).toBeVisible()
    await card.getByRole('button', { name: 'Preview the fix' }).click()
    await expect(card.getByText(/nothing written yet/)).toBeVisible()
    await expect(card.getByRole('button', { name: 'Repair now' })).toBeDisabled()
    expect(applies).toHaveLength(1)
    expect(applies[0]).toContain('"dry_run":true')
  })

  test('without GuestKit the card explains how to turn it on', async ({ page }) => {
    await page.route('**/api/v1/guest-repair/capabilities', (r) =>
      r.fulfill({ json: { cli_found: false, agent_binary: '', agent_binary_found: false } }))
    await page.goto(`/platform/vms/${VM}?action=bootdoctor`)
    await expect(page.getByTestId('boot-doctor-missing-guestkit')).toBeVisible({ timeout: 15_000 })
  })

  test('a powered-off machine can be compared with another powered-off machine (read-only)', async ({ page }) => {
    const bodies: string[] = []
    await page.route('**/guest-drift', (r) => {
      bodies.push(r.request().postData() ?? '')
      return r.fulfill({ json: { vm: 'vm-1', disk: '/d', action: 'drift', dry_run: true, backup: false, output: 'Drift score: 12% (below threshold)', exit_ok: true } })
    })
    await page.goto('/platform/vms/vm-1')
    const card = page.getByTestId('guest-drift')
    // The mock fleet lists db-01 as stopped, so it is offered as the baseline.
    await expect(card).toBeVisible({ timeout: 15_000 })
    await card.getByRole('combobox', { name: 'Machine to compare against' }).selectOption('db-01')
    await card.getByRole('button', { name: 'Compare' }).click()
    await expect(card.getByText(/Drift score: 12%/)).toBeVisible()
    expect(bodies[0]).toContain('"baseline":"db-01"')
  })
})

test.describe('Migration copilot', () => {
  test('scores machines and groups them into waves with what blocks each', async ({ page }) => {
    await mockPlatformApi(page)
    const plan = (score: number, boot: number, extra: object = {}) => ({
      image_path: '/d', target: 'kvm', migration_score: score, boot_score: boot, estimated_downtime_minutes: 5,
      driver_injections: [], required_changes: [], licensing_warnings: [], summary: '', ...extra,
    })
    let n = 0
    await page.route('**/guestkit/vms/*/migrate-plan*', (r) => {
      n += 1
      return r.fulfill({ json: n === 1 ? plan(92, 90) : plan(90, 40, { licensing_warnings: ['OEM Windows key'] }) })
    })
    await page.goto('/platform/migration?tab=waves')
    const planner = page.getByTestId('migration-wave-planner')
    await expect(planner).toBeVisible({ timeout: 15_000 })
    await planner.getByRole('button', { name: 'Score my machines' }).click()
    await expect(planner.locator('[data-wave="1"] [data-machine]').first()).toBeVisible({ timeout: 15_000 })
    await expect(planner.locator('[data-wave="3"]').getByText(/Licensing: OEM Windows key/).first()).toBeVisible()
  })
})

test.describe('Backup verification', () => {
  test('shows verified, failed and never-checked backups and can re-check one', async ({ page }) => {
    await mockPlatformApi(page)
    const rec = (id: string, over: object) => ({ id, vm_id: 'v1', backup_type: 'full', status: 'completed', backup_path: '/b/x.qcow2', created_at: '2026-10-01T00:00:00Z', verify_status: '', ...over })
    await page.route('**/api/v1/vms/*/backups', (r) => r.request().method() === 'GET'
      ? r.fulfill({ json: [
          rec('b1', { message: 'backup complete (application-consistent, verified)', verify_status: 'ok', verified_at: new Date().toISOString() }),
          rec('b2', { verify_status: 'failed', verify_message: 'vda.qcow2 failed its integrity check' }),
          rec('b3', {}),
        ] })
      : r.continue())
    const verifies: string[] = []
    await page.route('**/backups/*/verify', (r) => { verifies.push(r.request().url()); return r.fulfill({ json: { ok: true, message: '1 image(s) intact' } }) })
    await page.goto('/platform/vms/vm-1?tab=backup')
    const chips = page.getByTestId('backup-health')
    await expect(chips).toHaveCount(3, { timeout: 15_000 })
    await expect(chips.nth(0)).toContainText('Verified')
    await expect(chips.nth(0)).toContainText('application-consistent')
    await expect(chips.nth(1)).toContainText('Failed its check')
    await expect(chips.nth(2)).toContainText('Not verified yet')
    await page.getByRole('button', { name: 'Verify now' }).first().click()
    await expect.poll(() => verifies.length).toBe(1)
  })
})
