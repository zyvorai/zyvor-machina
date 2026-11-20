// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

const RUN = {
  answer: 'Two running machines have no completed backup: db-01 and web-01. I queued a backup for each.',
  steps: [
    { kind: 'tool_call', tool: 'list_vms', detail: '{"state":"running"}' },
    { kind: 'tool_result', tool: 'list_vms', detail: '{}' },
    { kind: 'tool_call', tool: 'propose_action', detail: '{"action":"create_backup","vm":"db-01"}' },
    { kind: 'tool_call', tool: 'propose_action', detail: '{"action":"create_backup","vm":"web-01"}' },
  ],
  proposed_action_ids: ['a', 'b'],
  provider: 'anthropic',
}

test.describe('Zyra agent panel', () => {
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page)
    // Older controller without the stream route: the panel must fall back to the single request.
    await page.route('**/ai/agent/stream', (route) => route.fulfill({ status: 404, body: 'not found' }))
  })

  test('asking shows the answer, the queued-proposals note and the step trace', async ({ page }) => {
    await page.route('**/ai/agent/run', (route) => route.fulfill({ json: RUN }))
    await page.goto('/platform/zyra/approvals')
    const panel = page.getByTestId('zyra-agent-panel')
    await expect(panel).toBeVisible({ timeout: 15_000 })
    await panel.getByText('Which machines have no backup?').click()
    await expect(panel.getByText(/db-01 and web-01/)).toBeVisible()
    await expect(panel.getByText('2 proposals added to the queue below')).toBeVisible()
    await panel.getByText(/How I got there/).click()
    await expect(panel.getByText('Queued a proposal').first()).toBeVisible()
  })

  test('a disabled AI provider shows the server message, not a blank panel', async ({ page }) => {
    await page.route('**/ai/agent/run', (route) =>
      route.fulfill({ status: 400, json: { error: 'Zyra AI is turned off — enable an AI provider in Settings → AI Providers' } }))
    await page.goto('/platform/zyra/approvals')
    const panel = page.getByTestId('zyra-agent-panel')
    await panel.getByText('Is anything unhealthy right now?').click()
    await expect(panel.getByRole('alert')).toContainText('turned off')
  })

  test('trust ladder offers automatic after a streak and saves the change', async ({ page }) => {
    const rows = [
      { action_type: 'create_backup', level: 'ask', max_per_run: 3, approved_streak: 6, total_approved: 6, total_rejected: 0, offer: true },
      { action_type: 'start_vm', level: 'ask', max_per_run: 3, approved_streak: 0, total_approved: 0, total_rejected: 0, offer: false },
    ]
    const puts: string[] = []
    await page.route('**/ai/trust**', async (route) => {
      if (route.request().method() === 'PUT') {
        puts.push(`${route.request().url().split('/').pop()} ${route.request().postData()}`)
        rows[0] = { ...rows[0], level: 'auto', offer: false }
      }
      await route.fulfill({ json: rows })
    })
    await page.goto('/platform/zyra/approvals')
    const ladder = page.getByTestId('zyra-trust-ladder')
    await expect(ladder).toBeVisible({ timeout: 15_000 })
    await expect(ladder.getByText(/approved this 6 times in a row/)).toBeVisible()
    await ladder.locator('[data-trust="create_backup"]').getByRole('button', { name: 'Automatic' }).click()
    await expect(ladder.locator('[data-trust="create_backup"]')).toHaveAttribute('data-level', 'auto')
    expect(puts[0]).toContain('create_backup')
    expect(puts[0]).toContain('"level":"auto"')
  })

  test('steps appear live while the agent works, then the answer', async ({ page }) => {
    const sse = (o: unknown) => `data: ${JSON.stringify(o)}\n\n`
    await page.route('**/ai/agent/stream', (route) =>
      route.fulfill({
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
        body:
          sse({ type: 'step', kind: 'tool_call', tool: 'find_idle_vms', detail: '{}' }) +
          sse({ type: 'step', kind: 'tool_result', tool: 'find_idle_vms', detail: '{}' }) +
          sse({ type: 'done', run: { ...RUN, answer: 'Nothing is idle yet.', steps: [], proposed_action_ids: [] } }),
      }))
    await page.goto('/platform/zyra/approvals')
    const panel = page.getByTestId('zyra-agent-panel')
    await panel.getByText('Is anything unhealthy right now?').click()
    await expect(panel.getByText('Nothing is idle yet.')).toBeVisible()
  })
})
