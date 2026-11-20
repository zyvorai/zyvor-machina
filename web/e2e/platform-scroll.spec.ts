// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'
import { mockAuthenticatedApi } from './helpers/authMock'
import { expectPageScrolls } from './helpers/platformTestHelpers'

const SHORT_VIEWPORT = 400

test.describe('platform pages document scroll', () => {
  test.describe.configure({ retries: 1 })
  for (const [path, heading] of [
    ['/platform/vms', /machine finder/i],
    ['/platform/hosts', /hosts/i],
    ['/platform/settings', /settings/i],
    ['/platform/network-canvas', /network canvas/i],
    ['/platform/templates', /marketplace/i],
    ['/platform/networks', /networks/i],
    ['/platform/zeus/security', /security center/i],
    ['/platform/vms?lens=topology', /machine finder|topology/i],
    ['/platform/zyra/rightsizing', /vm rightsizing/i],
    ['/platform/create-iso', /create vm from iso/i],
    ['/platform/create-advanced', /advanced vm install/i],
    ['/platform/events', /logs & audit/i],
    ['/platform/users', /Access & Workspaces/i],
    ['/platform/observability', /Observability/i],
    ['/platform/zeus/security/ports', /Open Ports/i],
  ] as const) {
    test(`${path} scrolls with short viewport`, async ({ page }) => {
      await mockPlatformApi(page, { tier: 'power' })
      if (path === '/platform/create-iso') {
        await page.route('**/api/v1/content/images**', async (route) => {
          await route.fulfill({
            json: [
              {
                id: 'iso-1',
                name: 'debian-12.iso',
                path: '/var/lib/libvirt/images/debian-12.iso',
                status: 'available',
                kind: 'iso',
              },
            ],
          })
        })
      }
      await page.goto(path)
      await expect(page.getByRole('heading', { level: 1, name: heading }).first()).toBeVisible({ timeout: 20_000 })
      await expectPageScrolls(page, { viewportHeight: SHORT_VIEWPORT })
    })
  }

  test('/platform/vms/:id scrolls on VM detail', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
    await page.goto('/platform/vms/v1')
    await expect(page.getByRole('heading', { name: /vm-1/i }).first()).toBeVisible({ timeout: 20_000 })
    await expectPageScrolls(page, { viewportHeight: SHORT_VIEWPORT })
  })

  test('/platform/vms/:id keeps detail tabs sticky while scrolling', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
    await page.goto('/platform/vms/v1')
    await expect(page.getByRole('tab', { name: 'Overview' })).toBeVisible({ timeout: 20_000 })
    await page.setViewportSize({ width: 1280, height: 400 })
    const before = await page.getByRole('tab', { name: 'Overview' }).boundingBox()
    await page.evaluate(() => window.scrollTo(0, 1200))
    const after = await page.getByRole('tab', { name: 'Overview' }).boundingBox()
    expect(before).not.toBeNull()
    expect(after).not.toBeNull()
    expect(after!.y).toBeLessThan(before!.y + 80)
    expect(after!.y).toBeLessThan(200)
  })

  test('/platform/vms/:id tab change keeps tabs reachable', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
    await page.goto('/platform/vms/v1')
    await expect(page.getByRole('tab', { name: 'Overview' })).toBeVisible({ timeout: 20_000 })
    await page.setViewportSize({ width: 1280, height: 400 })
    await page.evaluate(() => window.scrollTo(0, 800))
    await page.getByRole('tab', { name: 'Performance' }).click()
    await expect(page.getByRole('tab', { name: 'Performance' })).toHaveAttribute('aria-selected', 'true')
    await expect(page.locator('#platform-detail-tabs')).toBeVisible()
  })

  test('popout VM detail scrolls in popout shell', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
    await page.goto('/platform/vms/v1?popout=1')
    await expect(page.getByRole('heading', { name: /vm-1/i }).first()).toBeVisible({ timeout: 20_000 })
    await expectPageScrolls(page, { viewportHeight: SHORT_VIEWPORT })
  })

  test('/platform/hosts/:id scrolls on host detail', async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
    await page.goto('/platform/hosts/h1')
    await expect(page.getByRole('heading').first()).toBeVisible({ timeout: 20_000 })
    await expectPageScrolls(page, { viewportHeight: SHORT_VIEWPORT })
  })
})

test.describe('classic pages document scroll', () => {
  test('/vms scrolls with short viewport', async ({ page }) => {
    await mockAuthenticatedApi(page)
    await page.goto('/vms')
    await expect(page.getByRole('heading', { name: /virtual machines/i })).toBeVisible({ timeout: 15_000 })
    await expectPageScrolls(page, { viewportHeight: SHORT_VIEWPORT })
  })

  test('dashboard scrolls with short viewport', async ({ page }) => {
    await mockAuthenticatedApi(page)
    await page.goto('/')
    await expect(page.locator('#main-content')).toBeVisible({ timeout: 15_000 })
    await expectPageScrolls(page, { viewportHeight: SHORT_VIEWPORT })
  })

})
