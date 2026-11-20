// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockPlatformApi } from './platformMock'

test.describe('Platform ConsoleHub', () => {
  test.describe.configure({ retries: 1 })
  test.beforeEach(async ({ page }) => {
    await mockPlatformApi(page, { tier: 'power' })
  })

  test('opens Cinema mode by default from VM detail', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    await expect(page.locator('h1').filter({ hasText: 'vm-1' })).toBeVisible({ timeout: 15_000 })
    await page.getByRole('link', { name: /Open Cinema/i }).first().click()
    await expect(page).toHaveURL(/\/platform\/vms\/v1\/consolehub/)
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('Machina Cinema')).toBeVisible()
  })

  test('legacy /console redirects to consolehub', async ({ page }) => {
    await page.goto('/platform/vms/v1/console')
    await expect(page).toHaveURL(/\/platform\/vms\/v1\/consolehub/)
  })

  test('Cinema shows access note pill instead of checklist', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('access-note-pill')).toBeVisible()
    await expect(page.getByTestId('vm-laptop-access-checklist')).toHaveCount(0)
  })

  test('Cinema control strip and Ops Shelf handle', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('ops-shelf-handle')).toBeVisible()
  })

  test('Studio mode shows lens bar and serial recovery', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub?mode=studio')
    await expect(page.getByTestId('studio-layout')).toBeVisible({ timeout: 15_000 })
    await page.getByRole('button', { name: 'Serial' }).first().click()
    await expect(page.getByTestId('console-login-recovery')).toBeVisible()
    const createReq = page.waitForRequest(
      (req) => req.url().includes('/port-forwards') && req.method() === 'POST',
    )
    await page.getByTestId('console-login-recovery').getByRole('button', { name: 'Expose SSH' }).click()
    const req = await createReq
    expect(req.postDataJSON()).toMatchObject({ host_port: 2222, vm_port: 22 })
  })

  test('Studio shell lens shows guest access banners', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub?mode=studio')
    await page.getByRole('button', { name: 'Shell' }).first().click()
    await expect(page.getByTestId('guest-access-banner')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('shell-access-banner')).toBeVisible()
  })

  test('Ops Shelf opens from cinema handle', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.getByTestId('ops-shelf-handle').click()
    const panel = page.getByTestId('ops-shelf')
    await expect(panel).toBeVisible({ timeout: 15_000 })
    await expect(panel.getByTestId('vm-port-forward-panel')).toBeVisible()
    await expect(panel.getByTestId('consolehub-session-history')).toBeVisible()
    await panel.getByRole('button', { name: 'Health' }).click()
    await expect(panel.getByRole('button', { name: 'Run scan' })).toBeVisible()
  })

  test('KubeVirt VM serial in studio uses native console', async ({ page }) => {
    await page.goto('/platform/vms/kv1/consolehub?mode=studio')
    await expect(page.getByText('kv-vm-1').first()).toBeVisible({ timeout: 15_000 })
    await page.getByRole('button', { name: 'Serial' }).first().click()
    await expect(page.getByTestId('kubevirt-serial-console')).toBeVisible({ timeout: 15_000 })
  })

  test('SPICE VM shows WebRTC in cinema protocol chips', async ({ page }) => {
    await page.goto('/platform/vms/sp1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.getByRole('button', { name: 'webrtc spice', exact: true }).click()
    await expect(page.getByTestId('webrtc-spice-console')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('spice-audio-banner')).toBeVisible()
    await expect(page.getByTestId('spice-console-iframe')).toHaveAttribute('data-audio', 'on')
  })

  test('Cinema control strip hides after idle', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('cinema-control-strip')).toHaveAttribute('data-idle', 'false')
    await page.waitForTimeout(4000)
    await expect(page.getByTestId('cinema-control-strip')).toHaveAttribute('data-idle', 'true')
  })

  test('restores saved Studio mode when consolehub URL omits mode', async ({ page }) => {
    await page.addInitScript(() => {
      localStorage.setItem('machina-console-mode:v1', 'studio')
    })
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page).toHaveURL(/mode=studio/, { timeout: 15_000 })
    await expect(page.getByTestId('studio-layout')).toBeVisible({ timeout: 15_000 })
  })

  test('Cinema shows recording watermark when policy enabled', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('cinema-recording-badge')).toBeVisible()
    await expect(page.getByTestId('console-watermark')).toBeVisible()
  })

  test('spectator link enables read-only Cinema badges', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub?spectator=mock-spectator&session=00000000-0000-4000-8000-000000000002')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('cinema-readonly-badge')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('console-watermark')).toContainText('Read-only', { timeout: 15_000 })
  })

  test('Ops Shelf break-glass starts recorded session', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.getByTestId('ops-shelf-handle').click()
    const panel = page.getByTestId('ops-shelf')
    await expect(panel.getByTestId('ops-shelf-break-glass')).toBeVisible()
    await panel.getByPlaceholder('Reason (required for audit)').fill('Emergency access for prod outage')
    await panel.getByRole('button', { name: 'Start break-glass session' }).click()
    await expect(page.getByText(/Break-glass session started/i)).toBeVisible({ timeout: 10_000 })
  })

  test('Share view copies collaborative spectator link', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write'])
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.mouse.move(640, 480)
    await page.getByTestId('cinema-more').click()
    await page.getByTestId('cinema-share-view').click()
    await expect(page.getByText(/Collaborator link copied/i)).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('ops-shelf-handle').click()
    await expect(page.getByTestId('ops-shelf-collaborate')).toBeVisible()
  })

  test('Ops Shelf session history shows replay for recorded sessions', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.getByTestId('ops-shelf-handle').click()
    await expect(page.getByTestId('consolehub-replay-00000000')).toBeVisible()
  })

  test('Cinema clipboard panel opens from control strip', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write'])
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    // Move mouse and wait for idle timeout to reset before checking state
    await page.mouse.move(640, 480)
    await page.waitForTimeout(300)
    await expect(page.getByTestId('cinema-control-strip')).toHaveAttribute('data-idle', 'false', { timeout: 5_000 })
    await page.getByTestId('cinema-clipboard').click()
    await expect(page.getByTestId('cinema-clipboard-panel')).toBeVisible({ timeout: 5_000 })
    await expect(page.getByRole('button', { name: 'Send to VM' })).toBeVisible()
  })

  test('Ops Shelf file transfer panel builds scp guidance', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.getByTestId('ops-shelf-handle').click()
    await expect(page.getByTestId('ops-shelf-file-transfer')).toBeVisible()
  })

  test('SPICE Cinema enables browser audio on iframe', async ({ page }) => {
    await page.goto('/platform/vms/sp1/consolehub')
    await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
    await page.getByRole('button', { name: 'webrtc spice', exact: true }).click()
    await expect(page.getByTestId('webrtc-spice-console')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('spice-console-iframe')).toHaveAttribute('data-audio', 'on')
    await expect(page.getByTestId('cinema-spice-audio-badge')).toBeVisible()
  })

  test('Cinema multi-monitor chips appear for ultra-wide guest', async ({ page }) => {
    await page.goto('/platform/vms/v1/consolehub')
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await page.evaluate(() => {
      window.dispatchEvent(
        new CustomEvent('machina:console-guest-size', { detail: { width: 3840, height: 1080 } }),
      )
    })
    await page.waitForTimeout(200)
    await page.mouse.move(640, 480)
    await page.getByTestId('cinema-display').click()
    await expect(page.getByRole('button', { name: 'All monitors' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'M1' })).toBeVisible()
  })

  test('Cinema Hardware button opens libvirt hardware drawer', async ({ page }) => {
    const summaryReady = page.waitForResponse((r) => r.url().includes('/hardware-summary') && r.ok())
    await page.goto('/platform/vms/v1/consolehub')
    await summaryReady
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await page.mouse.move(640, 480)
    await page.getByTestId('cinema-hardware').click()
    const drawer = page.getByTestId('vm-hardware-drawer')
    await expect(drawer).toBeVisible({ timeout: 15_000 })
    await expect(drawer.getByTestId('vm-hardware-cpu')).toContainText('vCPU')
    await expect(drawer.getByTestId('vm-hardware-memory')).toContainText('GiB')
    await expect(drawer.getByTestId('hardware-badge-live')).toBeVisible()
    await drawer.getByTestId('vm-hardware-edit').click()
    await expect(page.getByTestId('vm-edit-hardware-drawer')).toBeVisible()
    await expect(page.getByTestId('vm-edit-hardware-section-cpu')).toBeVisible()
  })

  test('Hardware drawer runs compatibility check', async ({ page }) => {
    const summaryReady = page.waitForResponse((r) => r.url().includes('/hardware-summary') && r.ok())
    await page.goto('/platform/vms/v1/consolehub')
    await summaryReady
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await page.mouse.move(640, 480)
    await page.getByTestId('cinema-hardware').click()
    const drawer = page.getByTestId('vm-hardware-drawer')
    await expect(drawer).toBeVisible({ timeout: 15_000 })
    const compatReady = page.waitForResponse((r) => r.url().includes('/hardware-compat') && r.ok())
    const capsReady = page.waitForResponse((r) => r.url().includes('/domain-caps') && r.ok())
    await drawer.getByTestId('vm-hardware-compat-check').click()
    await compatReady
    await capsReady
    await expect(page.getByTestId('vm-hardware-compat-panel')).toContainText('Compatible with this host', {
      timeout: 15_000,
    })
    await expect(page.getByTestId('vm-domain-caps-summary')).toContainText('x86_64 / kvm')
    await expect(page.getByTestId('vm-domain-caps-summary')).toContainText('pc-q35-8.2')
  })

  test('Hardware drawer opens attach device browser', async ({ page }) => {
    const summaryReady = page.waitForResponse((r) => r.url().includes('/hardware-summary') && r.ok())
    const domainReady = page.waitForResponse((r) => r.url().includes('/domain-xml') && r.ok())
    await page.goto('/platform/vms/v1/consolehub')
    await summaryReady
    await domainReady
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await page.mouse.move(640, 480)
    await page.getByTestId('cinema-hardware').click()
    const drawer = page.getByTestId('vm-hardware-drawer')
    await expect(drawer).toBeVisible({ timeout: 15_000 })
    const nodeDevicesReady = page.waitForResponse(
      (r) => r.url().includes('/hosts/h1/libvirt') && r.url().includes('host.node_devices') && r.ok(),
    )
    await drawer.getByTestId('vm-hardware-attach-device').click()
    await nodeDevicesReady
    const attachDrawer = page.getByTestId('vm-hostdev-attach-drawer')
    await expect(attachDrawer).toBeVisible({ timeout: 15_000 })
    await expect(attachDrawer.getByTestId('vm-hostdev-attached-pci-0000-03-00-0')).toBeVisible()
    await expect(attachDrawer.getByTestId('vm-hostdev-row-pci_0000_06_00_0')).toContainText('IOMMU 14')
    await expect(attachDrawer.getByTestId('vm-hostdev-row-usb_1_2')).toBeVisible()
    await attachDrawer.getByRole('button', { name: 'Attach' }).first().click()
    await expect(page.getByText('Attached pci_0000_06_00_0 · 0000:06:00.0')).toBeVisible({ timeout: 15_000 })
  })

  test('KubeVirt VM detail shows read-only Hardware tab', async ({ page }) => {
    await page.goto('/platform/vms/kv1')
    // Two "Open Cinema" links coexist by design here: the kubevirt header action and
    // VmConsoleHeroPreview's own CTA (rendered on the overview tab for every VM).
    await expect(page.getByRole('link', { name: /Open Cinema/i }).first()).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('vm-detail-hardware')).toBeVisible()
    await page.getByRole('tab', { name: 'Hardware' }).click()
    await expect(page.getByTestId('vm-hardware-tab')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('vm-hardware-cpu')).toContainText('vCPU')
    await expect(page.getByTestId('vm-kubevirt-hardware-node')).toContainText('worker-1')
    await page.getByTestId('vm-detail-hardware').click()
    await expect(page.getByTestId('vm-kubevirt-hardware-drawer')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('vm-kubevirt-hardware-note')).toContainText('Running VM')
  })

  test('KubeVirt Cinema Hardware button opens read-only drawer', async ({ page }) => {
    await page.goto('/platform/vms/kv1/consolehub?mode=cinema')
    await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 15_000 })
    await page.mouse.move(640, 480)
    await page.getByTestId('cinema-hardware').click()
    const drawer = page.getByTestId('vm-kubevirt-hardware-drawer')
    await expect(drawer).toBeVisible({ timeout: 15_000 })
    await expect(drawer.getByTestId('vm-hardware-memory')).toContainText('GiB')
    await expect(drawer.getByTestId('vm-kubevirt-hardware-vmi')).toContainText('Running')
  })

  test('VM detail Hardware tab and action bar button', async ({ page }) => {
    await page.goto('/platform/vms/v1')
    await expect(page.getByTestId('vm-detail-hardware')).toBeVisible({ timeout: 20_000 })
    await page.getByRole('tab', { name: 'Hardware' }).click()
    await expect(page.getByTestId('vm-hardware-tab')).toBeVisible({ timeout: 15_000 })
    await expect(page.getByTestId('vm-hardware-cpu')).toContainText('vCPU')
    await page.getByTestId('vm-hardware-edit').click()
    await expect(page.getByTestId('vm-edit-hardware-drawer')).toBeVisible()
  })
})
