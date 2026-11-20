// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Serial feature-by-feature matrix (mocked API) — F01 through F18.

import { test, expect } from '@playwright/test'
import {
  applyGuestHealthDegradedOverride,
  openMockVmDetail,
  openPowerOverflow,
  setupMockPlatform,
} from './helpers/featureMatrix'

test.describe.configure({ mode: 'serial' })

test.beforeAll(async ({ browser }) => {
  // Warm browser for serial suite stability.
  const page = await browser.newPage()
  await page.close()
})

test.beforeEach(async ({ page }) => {
  await setupMockPlatform(page)
})

test('F01 — VM detail hero shows machine strip and status pills', async ({ page }) => {
  await openMockVmDetail(page)
  await expect(page.getByTestId('vm-detail-hero')).toBeVisible({ timeout: 15_000 })
  // VmDetailHero's own name/state/host header (incl. .vm-detail-hero-icon--live) was removed as a
  // duplicate of PlatformVmDetail's page header (fbe4638e) — the running state now shows there instead.
  await expect(page.getByText('running', { exact: true }).first()).toBeVisible()
  await expect(
    page.getByTestId('vm-detail-hero').getByText(/blocker|Ready to connect|Doctor \d+/i).first(),
  ).toBeVisible({ timeout: 10_000 })
})

test('F02 — action bar exposes Cinema, SSH, and power overflow', async ({ page }) => {
  await openMockVmDetail(page)
  const bar = page.getByTestId('vm-detail-action-bar')
  await expect(bar).toBeVisible({ timeout: 15_000 })
  await expect(bar.getByRole('link', { name: /Open Cinema/i })).toBeVisible()
  await expect(bar.getByRole('button', { name: 'SSH' })).toBeVisible()
  await openPowerOverflow(page)
  await expect(page.getByTestId('vm-force-reboot-button')).toBeVisible()
  // SideNav also has its own "Cloud-Init Studio" link, which substring-matches an unscoped "Studio" query.
  await expect(bar.getByRole('link', { name: 'Studio' })).toBeVisible()
})

test('F03 — attention stack chips swap expanded banner', async ({ page }) => {
  await applyGuestHealthDegradedOverride(page)
  await openMockVmDetail(page)
  const stack = page.getByTestId('vm-attention-stack')
  await expect(stack).toBeVisible({ timeout: 15_000 })
  await expect(stack.getByText('Guest agent setup', { exact: true })).toBeVisible()
  await stack.getByRole('button', { name: 'Expose SSH' }).click()
  await expect(stack.getByText('Laptop access', { exact: true })).toBeVisible()
})

test('F04 — Connect hub on Overview shows laptop path and copy actions', async ({ page }) => {
  await openMockVmDetail(page)
  await expect(page.getByTestId('vm-daily-access')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-laptop-access-checklist')).toBeVisible()
  const copyCmd = page.getByRole('button', { name: /Copy laptop cmd/i })
  await expect(copyCmd).toBeEnabled()
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write'])
  await copyCmd.click()
  await expect(page.getByRole('button', { name: /^Copied!$/i })).toBeVisible({ timeout: 5_000 })
})

test('F05 — Access tab shows full NAT panel and export section', async ({ page }) => {
  await openMockVmDetail(page, 'access')
  await expect(page.getByTestId('vm-daily-access')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Spec', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: /Spec \+ XML/i })).toBeVisible()
})

test('F06 — SSH connect dialog shows NAT banner and expose action', async ({ page }) => {
  await openMockVmDetail(page)
  await page.getByTestId('vm-detail-action-bar').getByRole('button', { name: 'SSH' }).click()
  await expect(page.getByTestId('vm-ssh-connect-dialog')).toBeVisible()
  await expect(page.getByTestId('vm-ssh-nat-banner')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Expose SSH & copy command' })).toBeVisible()
})

test('F07 — NAT expose SSH preset creates port-forward rule', async ({ page }) => {
  await page.goto('/platform/vms/v1?tab=network')
  await expect(page.getByTestId('vm-port-forward-panel')).toBeVisible({ timeout: 15_000 })
  const createReq = page.waitForRequest(
    (req) => req.url().includes('/port-forwards') && req.method() === 'POST',
  )
  await page.getByTestId('expose-service-ssh').click()
  const req = await createReq
  expect(req.postDataJSON()).toMatchObject({ host_port: 2222, vm_port: 22, protocol: 'tcp' })
})

test('F08 — Overview slim body shows usage bars and doctor one-liner', async ({ page }) => {
  await openMockVmDetail(page)
  await expect(page.getByTestId('vm-usage-bars')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/Doctor:/i)).toBeVisible()
  await expect(page.getByRole('button', { name: 'Full report →' })).toBeVisible()
})

test('F09 — Console tab guidance links to Cinema and Studio', async ({ page }) => {
  await openMockVmDetail(page, 'console')
  const consoleSection = page.locator('div.pt-2').filter({ has: page.getByRole('heading', { name: 'VNC console' }) })
  await expect(consoleSection).toBeVisible({ timeout: 15_000 })
  await expect(consoleSection.getByRole('link', { name: /Open Cinema/i })).toBeVisible()
  await expect(consoleSection.getByRole('link', { name: 'Studio', exact: true })).toBeVisible()
})

test('F10 — Doctor tab deep-links GuestKit migrate plan', async ({ page }) => {
  await openMockVmDetail(page, 'doctor')
  await expect(page.getByRole('button', { name: 'Run migrate plan' })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Run migrate plan' }).click()
  await expect(page).toHaveURL(/tab=guestHealth/)
})

test('F11 — Cinema ConsoleHub shell, ops shelf, and serial recovery', async ({ page }) => {
  test.setTimeout(60_000)
  await page.goto('/platform/vms/v1/consolehub?mode=studio')
  await expect(page.getByTestId('studio-layout')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'Serial' }).first().click()
  await expect(page.getByTestId('console-login-recovery')).toBeVisible({ timeout: 15_000 })
  await page.goto('/platform/vms/v1/consolehub?mode=cinema')
  await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 15_000 })
  await page.getByTestId('ops-shelf-handle').click()
  await expect(page.getByTestId('ops-shelf')).toBeVisible({ timeout: 15_000 })
})

test('F12 — Machine Finder table SSH and gallery Cinema', async ({ page }) => {
  await page.goto('/platform/vms?lens=table')
  await expect(page.getByRole('heading', { name: /Machine Finder/i })).toBeVisible({ timeout: 15_000 })
  await page.getByRole('button', { name: 'SSH' }).first().click()
  await expect(page.getByTestId('vm-ssh-connect-dialog')).toBeVisible()
  await page.goto('/platform/vms?lens=gallery')
  await expect(page.getByTestId('machine-finder-gallery')).toBeVisible({ timeout: 15_000 })
  await page.getByRole('link', { name: /Open Cinema/i }).first().click()
  await expect(page).toHaveURL(/\/consolehub/)
})

test('F13 — Mission Control command center opens SSH dialog', async ({ page }) => {
  // FleetCommandCenter only ever renders from Machine Finder (MachineFinderPage.tsx), not from the
  // bare Mission Control dashboard — go there directly. Every current caller overrides its default
  // testId prop ('fleet-command-center') — MachineFinderCommandCenter.tsx uses
  // 'machine-finder-command-center' instead, so that's what actually appears in the DOM.
  // The Machine Finder list fixture names this VM "web-01" (id v1).
  await page.goto('/platform/vms')
  await page.getByText('web-01').first().click({ timeout: 15_000 })
  await expect(page.getByTestId('machine-finder-command-center')).toBeVisible()
  await page.getByTestId('machine-finder-command-center').getByRole('button', { name: 'SSH' }).click()
  await expect(page.getByTestId('vm-ssh-connect-dialog')).toBeVisible()
  await expect(page.getByTestId('vm-ssh-nat-banner')).toBeVisible()
})

test('F14 — Compute panel opens CPU and memory modals', async ({ page }) => {
  await openMockVmDetail(page)
  await expect(page.getByRole('heading', { name: 'Compute' })).toBeVisible({ timeout: 15_000 })
  await expect(page.getByTestId('vm-usage-bars')).toBeVisible()
  await page.getByRole('button', { name: 'Edit CPU' }).click()
  await expect(page.getByText(/CPU topology — vm-1/i)).toBeVisible()
  await expect(page.getByText('Active vCPUs:')).toBeVisible()
  await page.getByRole('button', { name: 'Close' }).click()
  await page.getByRole('button', { name: 'Edit memory' }).click()
  await expect(page.getByText(/Memory — vm-1/i)).toBeVisible()
  await expect(page.getByText('Maximum memory (GiB)')).toBeVisible()
})

test('F15 — Disks tab attach and resize block device', async ({ page }) => {
  await openMockVmDetail(page, 'disks')
  await expect(page.getByTestId('vm-disks-panel')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('heading', { name: 'Libvirt disks' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Attach disk' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Resize block device' })).toBeVisible()
  const attachReq = page.waitForRequest(
    (req) => req.url().includes('/disks/attach') && req.method() === 'POST',
  )
  await page.getByRole('button', { name: 'Attach', exact: true }).click()
  const req = await attachReq
  expect(req.postDataJSON()).toMatchObject({ disk_path: expect.any(String), target_dev: expect.any(String) })
})

test('F16 — Network tab lists NICs and attach control', async ({ page }) => {
  await openMockVmDetail(page, 'network')
  await expect(page.getByTestId('vm-network-panel')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('heading', { name: 'Network interfaces' })).toBeVisible()
  await expect(page.getByText(/52:54:00:12:34:56|default/).first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('button', { name: 'Attach NIC' })).toBeVisible()
})

test('F17 — Snapshots tab create snapshot workflow', async ({ page }) => {
  await openMockVmDetail(page, 'snapshots')
  await expect(page.getByTestId('vm-snapshots-panel')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByPlaceholder('snap-01')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Create snapshot' })).toBeVisible()
})

test('F18 — Devices tab, CD-ROM inventory, and insert ISO', async ({ page }) => {
  await openMockVmDetail(page, 'devices')
  await expect(page.getByTestId('vm-devices-panel')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByRole('heading', { name: 'Shared directories (virtiofs)' })).toBeVisible()
  const isoScan = page.waitForResponse((r) => r.url().includes('/browse/isos') && r.ok())
  await openMockVmDetail(page, 'disks')
  await isoScan
  await expect(page.getByTestId('cdrom-eject-sda')).toBeVisible()
  await expect(page.getByTestId('vm-insert-iso-panel')).toBeVisible()
  await expect(page.getByTestId('vm-insert-iso-scan')).toBeVisible()
  await expect(page.getByTestId('vm-insert-iso-browse')).toBeEnabled()
  await page.getByTestId('vm-insert-iso-browse').click()
  await expect(page.getByRole('dialog', { name: 'Browse for ISO' })).toBeVisible()
  await page.getByRole('button', { name: '/var/lib/libvirt/images' }).click()
  await page.getByRole('button', { name: 'Select' }).click()
  await expect(page.getByRole('dialog', { name: 'Browse for ISO' })).toBeHidden()
  const insertReq = page.waitForRequest((req) => {
    if (req.method() !== 'POST' || !req.url().includes('/libvirt')) return false
    try {
      const body = req.postDataJSON() as { action?: string; payload?: { iso_path?: string } }
      return body.action === 'cdrom.insert'
    } catch {
      return false
    }
  })
  await page.getByTestId('vm-insert-iso-submit').click()
  const req = await insertReq
  const body = req.postDataJSON() as { payload: { iso_path: string; target: string } }
  expect(body.payload.iso_path).toContain('.iso')
  expect(body.payload.target).toBeTruthy()
})

test('F18b — CD-ROM eject Linux VM sends cdrom.eject with correct target', async ({ page }) => {
  await openMockVmDetail(page, 'disks')
  await expect(page.getByTestId('cdrom-eject-sda')).toBeVisible({ timeout: 15_000 })
  const ejectReq = page.waitForRequest((req) => {
    if (req.method() !== 'POST' || !req.url().includes('/libvirt')) return false
    try {
      const body = req.postDataJSON() as { action?: string; payload?: { target?: string } }
      return body.action === 'cdrom.eject'
    } catch { return false }
  })
  await page.getByTestId('cdrom-eject-sda').click()
  const req = await ejectReq
  const body = req.postDataJSON() as { payload: { target: string } }
  expect(body.payload.target).toBe('sda')
})

test('F19 — Windows VM CD-ROM insert and eject', async ({ page }) => {
  // override libvirt-details to return a Windows VM with a Windows ISO on sdb
  await page.route('**/api/v1/vms/*/libvirt-details', async (route) => {
    await route.fulfill({
      json: {
        name: 'win-server-2022',
        uuid: '00000000-0000-4000-8000-000000000002',
        state: 'running',
        vcpus: 4,
        memory_mb: 8192,
        os_type: 'hvm',
        arch: 'x86_64',
        autostart: true,
        persistent: true,
        interfaces: [{ mac_address: '52:54:00:ab:cd:ef', ip: '192.168.122.51', source: 'default', model: 'e1000e' }],
        disks: [
          { target: 'vda', device: 'disk', source: '/var/lib/libvirt/images/win-server-2022.qcow2', bus: 'virtio' },
          { target: 'sdb', device: 'cdrom', source: '/var/lib/libvirt/images/windows-server-2022.iso', bus: 'sata' },
        ],
        filesystems: [],
      },
    })
  })

  await openMockVmDetail(page, 'disks')
  await expect(page.getByTestId('vm-disks-panel')).toBeVisible({ timeout: 15_000 })

  // Windows ISO shows in disk inventory with eject button on target sdb
  await expect(page.getByTestId('cdrom-eject-sdb')).toBeVisible()
  await expect(page.getByText('windows-server-2022.iso')).toBeVisible()

  // Eject: verify cdrom.eject POST with target sdb
  const ejectReq = page.waitForRequest((req) => {
    if (req.method() !== 'POST' || !req.url().includes('/libvirt')) return false
    try {
      const b = req.postDataJSON() as { action?: string; payload?: { target?: string } }
      return b.action === 'cdrom.eject' && b.payload?.target === 'sdb'
    } catch { return false }
  })
  await page.getByTestId('cdrom-eject-sdb').click()
  await ejectReq

  // Insert panel is visible; select sdb from the CD-ROM target dropdown
  await expect(page.getByTestId('vm-insert-iso-panel')).toBeVisible()
  await page.getByLabel('CD-ROM target').selectOption('sdb')

  // Type a Windows ISO path and submit
  await page.getByRole('textbox', { name: 'ISO path' }).fill('/var/lib/libvirt/images/windows-server-2022.iso')
  const insertReq = page.waitForRequest((req) => {
    if (req.method() !== 'POST' || !req.url().includes('/libvirt')) return false
    try {
      const b = req.postDataJSON() as { action?: string; payload?: { iso_path?: string; target?: string } }
      return b.action === 'cdrom.insert' && (b.payload?.iso_path ?? '').includes('windows')
    } catch { return false }
  })
  await page.getByTestId('vm-insert-iso-submit').click()
  const insertBody = (await insertReq).postDataJSON() as { payload: { iso_path: string; target: string } }
  expect(insertBody.payload.iso_path).toContain('windows')
  expect(insertBody.payload.target).toBe('sdb')
})
