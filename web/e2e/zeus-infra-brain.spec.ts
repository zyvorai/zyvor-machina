// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test('Zeus Graph Brain tab loads and path analysis works', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra?tab=brain')
  await expect(page.getByText('Infrastructure Graph Brain')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/nodes · .* edges/)).toBeVisible()
  await page.getByRole('tab', { name: 'Graph Brain' }).click()
  await expect(page.getByText('Connectivity path')).toBeVisible()
})

test('Zeus rightsizing page loads recommendations', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/rightsizing')
  await expect(page.getByText('VM Rightsizing')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('idle-vm')).toBeVisible()
})

test('Incident commander page loads fleet RCA', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/incidents')
  await expect(page.getByText('Incident Commander')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Fleet RCA')).toBeVisible()
  await expect(page.getByText(/Network configuration change/)).toBeVisible()
})

test('Zeus approvals page lists pending actions', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra/approvals')
  await expect(page.getByText('Zyra approvals')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Restart idle-vm')).toBeVisible()
})

test('Zeus Fleet autonomous wizard dry-run shows plan steps', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra?tab=fleet')
  await page.getByRole('tab', { name: 'Fleet' }).click()
  await expect(page.getByRole('heading', { name: 'Autonomous run' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Dry-run plan' }).click()
  await expect(page.getByText('Identify idle VMs')).toBeVisible({ timeout: 10_000 })
  await expect(page.getByText('Queue rebalance moves')).toBeVisible()
})

test('Policy page downloads policy YAML', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/policy')
  await expect(page.getByRole('button', { name: 'Download policy YAML' })).toBeVisible({ timeout: 15_000 })
  const exportBody = await page.evaluate(async () => {
    const res = await fetch('/api/v1/platform/controller/api/v1/ai/policy/export', { credentials: 'same-origin' })
    const json = await res.json() as { yaml: string; rule_count: number }
    return { ok: res.ok, yaml: json.yaml, rule_count: json.rule_count }
  })
  expect(exportBody.ok).toBe(true)
  expect(exportBody.yaml).toContain('rules:')
  expect(exportBody.rule_count).toBe(1)
})

test('Stopped VM overview shows AI troubleshoot panel', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power', stoppedVm: true })
  await page.goto('/platform/vms/v1')
  await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('heading', { name: 'AI troubleshoot' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Run troubleshoot' }).click()
  await expect(page.getByText('Memory pressure')).toBeVisible({ timeout: 10_000 })
})

test('Zeus settings shows enterprise posture and editable prompts', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/settings?section=zyra')
  await expect(page.getByText('Enterprise Zyra posture')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('RCA template')).toBeVisible()
  await page.getByRole('button', { name: 'Edit' }).click()
  await page.locator('li.border input.input.text-sm').first().fill('Updated RCA')
  await page.locator('li.border').getByRole('button', { name: 'Save', exact: true }).click()
  await expect(page.getByText('Updated RCA')).toBeVisible({ timeout: 10_000 })
})

test('AI Providers settings shows task-class routing table', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/settings?section=ai-providers')
  await expect(page.getByRole('heading', { name: 'Task-class routing' })).toBeVisible({ timeout: 15_000 })
  await expect(
    page.locator('.tahoe-glass-card', { hasText: 'Task-class routing' }).getByText('Infrastructure', { exact: true }),
  ).toBeVisible()
  await expect(page.getByRole('button', { name: 'Save' }).first()).toBeVisible()
})

test('Graph Brain tab shows historical scrubber', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra?tab=brain')
  await expect(page.getByText('Time scrubber')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Graph at scrubber')).toBeVisible({ timeout: 10_000 })
})

test('Zeus Memory tab shows clear memory action', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra?tab=memory')
  await expect(page.getByRole('button', { name: 'Clear memory' })).toBeVisible({ timeout: 15_000 })
})

test('AI export and copilot stream mocks respond with credentials', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/reports')
  await expect(page.getByRole('heading', { name: 'Reports' })).toBeVisible({ timeout: 15_000 })

  const [csv, stream] = await page.evaluate(async () => {
    const csvRes = await fetch('/api/v1/platform/controller/api/v1/ai/cost/export.csv', { credentials: 'same-origin' })
    const csvText = await csvRes.text()
    const streamRes = await fetch('/api/v1/platform/controller/api/v1/ai/copilot/stream', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ message: 'How is the fleet?' }),
    })
    const streamText = await streamRes.text()
    return [
      { ok: csvRes.ok, text: csvText },
      { ok: streamRes.ok, text: streamText },
    ] as const
  })

  expect(csv.ok).toBe(true)
  expect(csv.text).toContain('metric,value')
  expect(stream.ok).toBe(true)
  expect(stream.text).toContain('Fleet looks healthy')
  expect(stream.text).toContain('"type":"chunk"')
})
