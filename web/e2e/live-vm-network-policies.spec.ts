// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials } from './helpers/liveAuth'

/** Read-only against a running daemon: validate is a dry run, trace changes nothing. */
const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
test.skip(!live || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER and PLAYWRIGHT_LIVE_PASS')

const PAGE = '/platform/zyra/security/network-policies'

test('live VM network policies page: status, dry-run validate, trace, flows', async ({ page }) => {
  await ensureLoggedIn(page, live!, PAGE)
  await expect(page.getByRole('heading', { name: 'VM Network Policies' })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByText(/Absent · native|Present/)).toBeVisible()

  await page.goto(`${live}${PAGE}?tab=editor`)
  await page.getByLabel('Template').selectOption('services')
  await page.getByRole('button', { name: 'Validate & preview' }).click()
  await expect(page.getByText('Valid', { exact: true })).toBeVisible({ timeout: 20_000 })
  await expect(page.getByText(/selects no service/).first()).toBeVisible()
  await page.getByLabel('Policy in plain English').fill('block everyone from reaching the internet')
  await page.getByRole('button', { name: 'Draft policy' }).click()
  await expect(page.getByLabel('Policy YAML')).toHaveValue(/nl-deny-/, { timeout: 30_000 })
  await expect(page.getByText(/Drafted by the sentence parser/)).toBeVisible()
  await expect(page.getByText(/Safe for|Would break/).first()).toBeVisible()

  await page.goto(`${live}${PAGE}?tab=tester`)
  await page.locator('#tr-from').fill('10.9.9.1')
  await page.locator('#tr-to').fill('10.9.9.2')
  await page.locator('#tr-port').fill('80')
  await page.getByRole('button', { name: 'Trace' }).click()
  await expect(page.getByText(/^(ALLOWED|DENIED)/)).toBeVisible({ timeout: 20_000 })

  await page.goto(`${live}${PAGE}?tab=flows`)
  const term = page.getByLabel('Packet flow terminal')
  await expect(term.getByText(/connected — streaming flows/)).toBeVisible({ timeout: 20_000 })

  await page.goto(`${live}${PAGE}?tab=map`)
  await expect(page.getByText(/endpoints · \d+ links|No flows in this window/)).toBeVisible({ timeout: 20_000 })
  await page.goto(`${live}${PAGE}?tab=learn`)
  await page.getByRole('button', { name: 'Generate' }).click()
  await expect(page.getByText(/policies from \d+ flow edges/)).toBeVisible({ timeout: 20_000 })
  await page.goto(`${live}${PAGE}?tab=alerts`)
  await expect(page.getByRole('table', { name: 'Flow alerts' }).or(page.getByText(/No alerts/))).toBeVisible({ timeout: 20_000 })
  await expect(page.getByRole('table', { name: 'Threat feeds' }).or(page.getByText(/No threat feeds/))).toBeVisible()
  await page.goto(`${live}${PAGE}?tab=endpoints`)
  await expect(page.getByRole('table', { name: 'Quarantined VMs' }).or(page.getByText('No VM is quarantined.'))).toBeVisible({ timeout: 20_000 })
  await page.goto(`${live}${PAGE}?tab=policies`)
  await expect(page.getByRole('button', { name: 'Grant access' })).toBeVisible({ timeout: 20_000 })
  await expect(page.getByRole('table', { name: 'Temporary access' }).or(page.getByText('No temporary access.'))).toBeVisible()

  for (const scope of ['fleet']) {
    await page.goto(`${live}${PAGE}?tab=policies&scope=${scope}`)
    await expect(page.getByText(/Managed by/)).toBeVisible({ timeout: 20_000 })
    await expect(page.getByRole('button', { name: 'Request access' })).toBeVisible()
    await expect(page.getByRole('table', { name: 'Temporary access' }).or(page.getByText('No temporary access.'))).toBeVisible()
    await expect(page.getByRole('alert')).toHaveCount(0)
  }
})

test('live VM network policies: projects view and sealed evidence export', async ({ page }) => {
  await ensureLoggedIn(page, live!, `${PAGE}?scope=fleet&tab=projects`)
  await expect(page.getByText('Default project isolation')).toBeVisible({ timeout: 30_000 })
  await expect(page.getByLabel('Default isolation')).toBeVisible()
  await expect(page.getByText('Egress IPs on hosts')).toBeVisible()
  await expect(page.getByLabel('Assign a VM')).toBeVisible()
  await expect(page.getByLabel('Preview changes')).not.toBeChecked()
  for (const scope of ['fleet', 'host']) {
    await page.goto(`${live}${PAGE}?scope=${scope}`)
    const [dl] = await Promise.all([
      page.waitForEvent('download', { timeout: 60_000 }),
      page.getByRole('button', { name: 'Export evidence as JSON' }).click(),
    ])
    const fs = await import('node:fs/promises')
    const doc = JSON.parse(await fs.readFile((await dl.path())!, 'utf8')) as { kind: string; digest: string; source: string; signature?: { alg: string } }
    expect(doc.kind).toBe('machina.io/segmentation-evidence/v1')
    expect(doc.digest).toMatch(/^[0-9a-f]{64}$/)
    expect(doc.source).toBe(scope === 'fleet' ? 'machina-controller' : 'machina-daemon')
    expect(doc.signature?.alg).toBe(scope === 'fleet' ? 'ecdsa-p256-sha256' : undefined)
  }
})
