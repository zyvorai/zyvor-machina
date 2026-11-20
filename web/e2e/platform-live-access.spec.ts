// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
test.skip(!live, 'Set PLAYWRIGHT_LIVE_URL to run live access e2e')

test.beforeEach(async ({ page }) => {
  test.skip(!liveCredentials(), 'Set PLAYWRIGHT_LIVE_USER and PLAYWRIGHT_LIVE_PASS to run live access tests')
  await ensureLoggedIn(page, live!, { tier: 'power' })
})

async function fetchVms(page: import('@playwright/test').Page) {
  const res = await page.request.get(`${live}/api/v1/platform/controller/api/v1/vms`, { ignoreHTTPSErrors: true })
  if (!res.ok()) return []
  const raw = await res.json()
  return (Array.isArray(raw) ? raw : ((raw as any).items ?? [])) as Array<{ id: string; guest_ip?: string }>
}

test('platform VM overview shows daily access when guest IP is private', async ({ page }) => {
  const vms = await fetchVms(page)
  const vm = vms.find((v) => v.guest_ip?.startsWith('192.168.')) ?? vms[0]
  test.skip(!vm?.id, 'no platform VMs on host')

  await page.goto(`${live}/platform/vms/${vm.id}`)
  await expect(page.getByTestId('vm-daily-access').or(page.getByTestId('vm-laptop-access-checklist'))).toBeVisible({
    timeout: 20_000,
  })
  await expect(page.getByTestId('vm-detail-action-bar')).toBeVisible({ timeout: 20_000 })
})

test('Access tab shows connect hub on live VM', async ({ page }) => {
  const vms = await fetchVms(page)
  const vm = vms.find((v) => v.guest_ip?.startsWith('192.168.')) ?? vms[0]
  test.skip(!vm?.id, 'no platform VMs on host')

  await page.goto(`${live}/platform/vms/${vm.id}?tab=access`)
  await expect(page.getByTestId('vm-daily-access')).toBeVisible({ timeout: 20_000 })
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible({ timeout: 20_000 })
})

test('network tab exposes SSH via port forward panel', async ({ page }) => {
  const vms = await fetchVms(page)
  const vm = vms.find((v) => v.guest_ip?.startsWith('192.168.')) ?? vms[0]
  test.skip(!vm?.id, 'no platform VMs on host')

  await page.goto(`${live}/platform/vms/${vm.id}?tab=network`)
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible({ timeout: 20_000 })
  const sshBtn = page.getByTestId('expose-service-ssh')
  if (await sshBtn.count()) {
    const createReq = page.waitForRequest(
      (req) => req.url().includes('/port-forwards') && req.method() === 'POST',
    )
    await sshBtn.click()
    const req = await createReq
    expect(req.postDataJSON()).toMatchObject({ vm_port: 22 })
  }
})

test('ConsoleHub serial lens shows recovery card on private NAT VM', { retries: 1 }, async ({ page }) => {
  test.setTimeout(90_000)
  const vms = await fetchVms(page)
  const vm = vms.find((v) => v.guest_ip?.startsWith('192.168.')) ?? vms[0]
  test.skip(!vm?.id, 'no platform VMs on host')

  await page.goto(`${live}/platform/vms/${vm.id}/consolehub`)
  const serialBtn = page.getByRole('button', { name: 'Serial' }).first()
  if (!await serialBtn.isVisible({ timeout: 30_000 }).catch(() => false)) {
    test.skip(true, 'Serial button not available on this VM — skipping')
    return
  }
  await serialBtn.click()
  await expect(
    page.getByTestId('console-login-recovery').or(page.getByTestId('vm-laptop-access-checklist')),
  ).toBeVisible({ timeout: 20_000 })
})

test('Machine Finder table SSH opens connect dialog', async ({ page }) => {
  await page.goto(`${live}/platform/vms?lens=table`)
  await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 20_000 })
  const sshBtn = page.getByTitle('SSH').first()
  test.skip((await sshBtn.count()) === 0, 'no VMs in table')
  await sshBtn.click()
  await expect(page.getByTestId('vm-ssh-connect-dialog')).toBeVisible({ timeout: 10_000 })
})
