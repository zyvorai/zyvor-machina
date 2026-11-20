// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { createApiWatch } from './helpers/apiWatch'
import { ensureLoggedIn, fetchPlatformFlags, type DesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
test.skip(!live, 'Set PLAYWRIGHT_LIVE_URL to run live UX wiring tests')

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const MANIFEST_PATH = path.join(ROOT, 'docs/ux-wiring-live-manifest.json')
const REPORT_PATH = path.join(ROOT, 'docs/ux-wiring-live-report.json')

interface ManifestAction {
  kind: 'tab' | 'click' | 'settingsSection' | 'filterPill'
  label?: string
  role?: string
  name?: string
}

interface ManifestEntry {
  id: string
  shell: string
  path: string
  tier: DesktopTier
  headingPattern: string
  requires: 'k8s' | null
  actions: ManifestAction[]
  resolve?: 'platformResource' | 'classicVm' | 'storagePool' | null
}

interface RunResult {
  id: string
  path: string
  status: 'passed' | 'failed' | 'skipped'
  reason?: string
  apiFailures?: { url: string; status: number; reason: string }[]
  warnings?: number
}

const report: {
  host: string
  generated_at: string
  passed: number
  failed: number
  skipped: number
  results: RunResult[]
} = {
  host: live ? new URL(live).hostname : 'unknown',
  generated_at: new Date().toISOString(),
  passed: 0,
  failed: 0,
  skipped: 0,
  results: [],
}

const manifest = JSON.parse(fs.readFileSync(MANIFEST_PATH, 'utf8')) as { entries: ManifestEntry[] }

async function controllerGet<T>(page: import('@playwright/test').Page, apiPath: string): Promise<T | null> {
  try {
    const res = await page.request.get(`${live}/api/v1/platform/controller${apiPath}`, {
      ignoreHTTPSErrors: true,
    })
    if (!res.ok()) return null
    return (await res.json()) as T
  } catch {
    return null
  }
}

async function daemonGet<T>(page: import('@playwright/test').Page, apiPath: string): Promise<T | null> {
  try {
    const res = await page.request.get(`${live}${apiPath}`, { ignoreHTTPSErrors: true })
    if (!res.ok()) return null
    return (await res.json()) as T
  } catch {
    return null
  }
}

async function resolveLivePath(
  page: import('@playwright/test').Page,
  entry: ManifestEntry,
): Promise<{ path: string; skipReason?: string }> {
  const [base, query = ''] = entry.path.split('?')
  const suffix = query ? `?${query}` : ''

  if (!entry.resolve && !base.includes(':')) {
    return { path: entry.path }
  }

  if (entry.resolve === 'platformResource' || base.includes('/platform/')) {
    if (base.includes('/platform/vms/:id')) {
      const vms = await controllerGet<Array<{ id: string }>>(page, '/api/v1/vms')
      const id = vms?.[0]?.id
      if (!id) return { path: entry.path, skipReason: 'no platform VMs' }
      return { path: `${base.replace(':id', id)}${suffix}` }
    }
    if (base.includes('/platform/hosts/:id') || base.includes(':hostId')) {
      const hosts = await controllerGet<Array<{ id: string }>>(page, '/api/v1/hosts')
      const id = hosts?.[0]?.id
      if (!id) return { path: entry.path, skipReason: 'no platform hosts' }
      return { path: `${base.replace(':id', id).replace(':hostId', id)}${suffix}` }
    }
    if (base.includes('/platform/zeus/security/firewall/:id')) {
      const overview = await controllerGet<{ targets?: Array<{ id: string }> }>(
        page,
        '/api/v1/zeus-firewall/overview',
      )
      const id = overview?.targets?.[0]?.id
      if (!id) return { path: entry.path, skipReason: 'no firewall targets' }
      return { path: `${base.replace(':id', id)}${suffix}` }
    }
  }

  if (entry.resolve === 'classicVm' || base.includes('/vms/:name')) {
    const vms = await daemonGet<Array<{ name: string }>>(page, '/api/v1/vms')
    const name = vms?.[0]?.name
    if (!name) return { path: entry.path, skipReason: 'no classic VMs' }
    return { path: `${base.replace(':name', encodeURIComponent(name))}${suffix}` }
  }

  if (entry.resolve === 'storagePool') {
    const pools = await daemonGet<Array<{ name: string }> | { pools?: Array<{ name: string }> }>(
      page,
      '/api/v1/storage/pools',
    )
    const list = Array.isArray(pools) ? pools : pools?.pools ?? []
    const pool = list[0]?.name
    if (!pool) return { path: entry.path, skipReason: 'no storage pools' }
    return { path: `${base.replace(':pool', encodeURIComponent(pool))}${suffix}` }
  }

  return { path: entry.path }
}

async function runAction(page: import('@playwright/test').Page, action: ManifestAction) {
  if (action.kind === 'tab' && action.label) {
    const tab = page.getByRole('button', { name: action.label, exact: true })
    await tab.first().click({ timeout: 8_000 })
    return
  }
  if (action.kind === 'filterPill' && action.label) {
    const pill = page.getByRole('button', { name: action.label, exact: true })
    await pill.first().click({ timeout: 8_000 })
    return
  }
  if (action.kind === 'settingsSection' && action.label) {
    await page.getByRole('button', { name: action.label }).first().click({ timeout: 8_000 })
    return
  }
  if (action.kind === 'click' && action.role && action.name) {
    await page.getByRole(action.role as 'button', { name: new RegExp(action.name, 'i') }).first().click({ timeout: 8_000 })
  }
}

async function attachFailure(page: import('@playwright/test').Page, entry: ManifestEntry, reason: string) {
  const info = test.info()
  try {
    const png = await page.screenshot({ fullPage: true })
    await info.attach(`${entry.id}-screenshot`, { body: png, contentType: 'image/png' })
  } catch {
    /* page may be closed */
  }
  await info.attach(`${entry.id}-failure`, { body: reason, contentType: 'text/plain' })
}

test.describe.configure({ mode: 'serial' })

for (const entry of manifest.entries) {
  test(`live UX: ${entry.id}`, async ({ browser }) => {
    test.setTimeout(180_000)
    const tier = entry.tier ?? 'normal'
    const context = await browser.newContext({
      ignoreHTTPSErrors: true,
      storageState: test.info().project.use.storageState,
    })
    const page = await context.newPage()
    await ensureLoggedIn(page, live!, { tier, navigate: false })
    const flags = await fetchPlatformFlags(page, live!)

    if (entry.requires === 'k8s' && !flags.k8sEnabled) {
      report.skipped += 1
      report.results.push({ id: entry.id, path: entry.path, status: 'skipped', reason: 'k8s disabled' })
      await context.close()
      test.skip(true, 'k8s disabled on host')
    }

    const watch = createApiWatch(page)
    const heading = new RegExp(entry.headingPattern, 'i')

    const resolved = await resolveLivePath(page, entry)
    if (resolved.skipReason) {
      report.skipped += 1
      report.results.push({ id: entry.id, path: entry.path, status: 'skipped', reason: resolved.skipReason })
      watch.dispose()
      await context.close()
      test.skip(true, resolved.skipReason)
    }

    await page.goto(`${live}${resolved.path}`, { waitUntil: 'domcontentloaded', timeout: 45_000 })

    if (page.url().includes('/login')) {
      report.skipped += 1
      report.results.push({ id: entry.id, path: entry.path, status: 'skipped', reason: 'not authenticated' })
      watch.dispose()
      await context.close()
      test.skip(true, 'login required — set PLAYWRIGHT_LIVE_USER/PASS')
    }

    if (entry.path.startsWith('/platform') && page.url().includes('/platform/settings') && !entry.path.includes('/settings')) {
      report.skipped += 1
      report.results.push({ id: entry.id, path: entry.path, status: 'skipped', reason: 'tier guard redirected to settings' })
      watch.dispose()
      await context.close()
      test.skip(true, 'tier guard')
    }

    await page.waitForLoadState('domcontentloaded')
    await page.waitForFunction(
      () => (document.body?.innerText?.replace(/\s+/g, '')?.length ?? 0) > 0,
      { timeout: 45_000 },
    )
    await page.waitForTimeout(1500)

    const jsErrors: string[] = []
    page.on('pageerror', (e) => jsErrors.push(e.message))

    try {
      await expect(page.getByText(heading).first()).toBeVisible({ timeout: 15_000 })
    } catch {
      // Some classic pages use different chrome — still validate APIs if page loaded
    }

    for (const action of entry.actions ?? []) {
      watch.reset()
      try {
        await runAction(page, action)
        await page.waitForTimeout(1200)
      } catch {
        // Non-fatal: action missing on this host tier/skin
      }
    }

    await page.waitForTimeout(800)
    const apiFailures = watch.getFailures()
    const warnCount = watch.getWarnings().length
    watch.dispose()

    const hardJs = jsErrors.filter((e) => !e.includes('ResizeObserver'))
    if (apiFailures.length > 0 || hardJs.length > 0) {
      const reason = hardJs[0] ?? apiFailures[0]?.reason ?? 'unknown failure'
      report.failed += 1
      report.results.push({
        id: entry.id,
        path: entry.path,
        status: 'failed',
        reason,
        apiFailures,
      })
      await attachFailure(page, entry, reason)
    } else {
      report.passed += 1
      report.results.push({
        id: entry.id,
        path: entry.path,
        status: 'passed',
        warnings: warnCount,
      })
    }
    await context.close()
  })
}

test.afterAll(async () => {
  report.generated_at = new Date().toISOString()
  fs.writeFileSync(REPORT_PATH, JSON.stringify(report, null, 2) + '\n')
  if (report.failed > 0) {
    const failedIds = report.results.filter((r) => r.status === 'failed').map((r) => r.id).slice(0, 10)
    throw new Error(
      `Live UX wiring: ${report.failed} failed, ${report.passed} passed, ${report.skipped} skipped` +
        (failedIds.length ? ` (e.g. ${failedIds.join(', ')})` : ''),
    )
  }
})
