// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

async function expectRouteVisible(
  page: import('@playwright/test').Page,
  path: string,
  heading: string | RegExp,
) {
  await page.goto(path)
  await expect(page.getByRole('heading', { name: heading }).first()).toBeVisible({ timeout: 20_000 })
}

const NORMAL_ROUTES: Array<{ path: string; heading: string | RegExp }> = [
  // MissionControlBriefing.tsx's headline text, not a cluster/host name (that only ever showed up
  // in a "Fleet summary unavailable" fallback body line, never the heading itself).
  { path: '/platform', heading: /Fleet at a glance|Scanning fleet|Control plane unreachable/i },
  { path: '/platform/vms', heading: 'Machine Finder' },
  { path: '/platform/hosts', heading: 'Hosts' },
  // /platform/integrations now redirects into Settings (App.tsx) and renders embedded there —
  // "Apps & Integrations" is no longer its own page heading; "Settings" is what's on screen.
  { path: '/platform/integrations', heading: 'Settings' },
  { path: '/platform/settings', heading: /Settings|General/i },
  { path: '/platform/backups', heading: 'Backups' },
  { path: '/platform/storage', heading: 'Storage' },
]

const ADVANCED_ROUTES: Array<{ path: string; heading: string | RegExp }> = [
  { path: '/platform/infrastructure', heading: 'Infrastructure' },
  { path: '/platform/operations', heading: 'Operations' },
  { path: '/platform/policy', heading: 'Policy & Quotas' },
  { path: '/platform/events', heading: 'Logs & Audit' },
  { path: '/platform/zeus/security/policies', heading: 'Policy Studio' },
  { path: '/platform/recommendations', heading: 'Recommendations' },
  { path: '/platform/observability', heading: 'Observability' },
]

const POWER_ROUTES: Array<{ path: string; heading: string | RegExp }> = [
  { path: '/platform/observability', heading: 'Observability' },
  { path: '/platform/placement', heading: 'Placement & HA' },
  { path: '/platform/policy', heading: 'Policy & Quotas' },
  { path: '/platform/api-keys', heading: /API keys/i },
  { path: '/platform/infrastructure', heading: 'Infrastructure' },
]

test.describe('power tier platform routes', () => {
  for (const { path, heading } of POWER_ROUTES) {
    test(`${path} loads without JS crash`, async ({ page }) => {
      const errors: string[] = []
      page.on('pageerror', (err) => errors.push(err.message))
      await mockPlatformApi(page, { tier: 'power' })
      await expectRouteVisible(page, path, heading)
      expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
    })
  }
})

test.describe('normal tier platform routes', () => {
  for (const { path, heading } of NORMAL_ROUTES) {
    test(`${path} loads without JS crash`, async ({ page }) => {
      const errors: string[] = []
      page.on('pageerror', (err) => errors.push(err.message))
      await mockPlatformApi(page, { tier: 'normal' })
      await expectRouteVisible(page, path, heading)
      expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
    })
  }
})

test.describe('advanced tier platform routes', () => {
  for (const { path, heading } of ADVANCED_ROUTES) {
    test(`${path} loads without JS crash`, async ({ page }) => {
      const errors: string[] = []
      page.on('pageerror', (err) => errors.push(err.message))
      await mockPlatformApi(page, { tier: 'advanced' })
      await expectRouteVisible(page, path, heading)
      expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
    })
  }
})

test('integrations hub lists Fleet Cloud', async ({ page }) => {
  // /platform/integrations redirects to /platform/settings?section=integrations, where
  // PlatformIntegrations renders `embedded` — its own "Apps & Integrations" heading is dropped,
  // "Settings" is the page's H1.
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/integrations')
  await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible()
  await expect(page.locator('a[href="/fleet-cloud"]').getByText('Fleet Cloud', { exact: true })).toBeVisible()
  // Scoped past the persistent SideNav's own "Kubernetes" section link, which is on every page.
  await expect(page.getByRole('link', { name: 'Kubernetes KubeVirt workloads' })).toBeVisible()
})

test('integrations hub shows live Fleet Cloud and K8s preview stats', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/integrations')
  await expect(page.getByText('Fleet Cloud preview')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Kubernetes preview')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Instances').first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('web-01')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('k3s')).toBeVisible({ timeout: 15_000 })
})

test('integrations hub lists classic Machina tools', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/integrations')
  await expect(page.getByText('Classic Machina tools')).toBeVisible()
  await expect(page.locator('a[href="/import"]').getByText('Import VM')).toBeVisible()
  await expect(page.locator('a[href="/node"]').getByText('Node & libvirt')).toBeVisible()
  await expect(page.getByText('Leaving the desktop')).toBeVisible()
})

test('Go menu navigates without tier bounce on allowed route', { retries: 1 }, async ({ page }) => {
  // The old menubar's "Go" destination menu is gone; spotlight (Ctrl+K) is the live way to jump
  // straight to a hub (see the equivalent rewritten test in platform-nav-coverage.spec.ts).
  test.setTimeout(90_000)
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform')
  await expect(page.getByTestId('mission-control-briefing')).toBeVisible({ timeout: 30_000 })
  await page.keyboard.press('Control+k')
  const spotlight = page.locator('.liquid-glass-modal-backdrop').filter({
    has: page.getByPlaceholder(/Zyra/i),
  })
  await spotlight.getByPlaceholder(/Zyra/i).fill('machine finder')
  await spotlight.getByRole('button', { name: /Machine Finder/i }).first().click()
  await expect(page).toHaveURL(/\/platform\/vms/)
})

// The old menubar's "View" menu (with tier-gated destinations like "Zeus OS", "Activity Monitor",
// "Finder") is unimported dead code (PlatformMacAppMenus.tsx) — the current GlobalBar has no
// equivalent menu to gate, so there's nothing left here to test.

test('backups destinations tab loads at normal tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'normal' })
  await page.goto('/platform/backups?tab=destinations')
  await expect(page.getByRole('heading', { name: 'Backup destinations' })).toBeVisible({ timeout: 15_000 })
})

test('vm detail topology tab loads at power tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/vms/v1?tab=topology')
  await expect(page.getByRole('heading', { name: 'vm-1' }).first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('2 nodes · 1 edges')).toBeVisible({ timeout: 15_000 })
})

test('firewall policy studio multisite panel loads at advanced tier', async ({ page }) => {
  await mockPlatformApi(page, { tier: 'advanced' })
  await page.goto('/platform/zeus/security/policies')
  await expect(page.getByRole('heading', { name: 'Multi-site DR' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Export federation bundle' })).toBeVisible()
})

test('developer route renders in place at power tier', async ({ page }) => {
  // Tier only shapes dock/sidebar density; gated routes render in place (a8bef254).
  // The subtitle text this used to assert is only the pre-load fallback (PlatformDeveloper.tsx);
  // once `getDeveloperOverview()` resolves — near-instant against the mock — it's replaced by a
  // stats line, so the fallback text isn't a reliable thing to wait for. The heading is.
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/developer')
  await expect(page.getByRole('heading', { name: 'Developer' })).toBeVisible({ timeout: 15_000 })
  await expect(page).toHaveURL(/\/platform\/developer/)
})

test('zyra fleet tab loads without JS crash', async ({ page }) => {
  // The earlier reported hang here was a misdiagnosis: this test used `/platform/zeus?tab=fleet`,
  // which doesn't route anywhere (the Zeus OS AI hub is `/platform/zyra` — `/platform/zeus` is only
  // a live prefix for `/platform/zeus/security*`), so it 404'd to PlatformNotFound, not the fleet
  // tab. On the *correct* URL the tab renders fine. Investigating did surface one real bug, fixed
  // separately: PlatformZyraOs.tsx's loadFleet threw on a summary response with a non-numeric
  // aggregate_monthly_usd (e.g. this mock's empty-array fallback for the unmocked endpoint),
  // silently swallowing the successfully-loaded heatmap/rebalance data behind a cryptic JS error.
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/zyra?tab=fleet')
  await expect(page.getByRole('heading', { name: 'Machina Zyra OS' })).toBeVisible({ timeout: 20_000 })
  await expect(page.getByText('Fleet Linux health')).toBeVisible({ timeout: 15_000 })
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
})

test('network canvas loads without JS crash', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))
  await mockPlatformApi(page, { tier: 'power' })
  await page.goto('/platform/network-canvas')
  await expect(page.getByRole('heading', { name: 'Network canvas' })).toBeVisible({ timeout: 20_000 })
  expect(errors.filter((e) => !e.includes('ResizeObserver'))).toEqual([])
})
