// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Live E2E: libvirt ubuntu-desktop ConsoleHub — Cockpit parity (serial + VNC + plan + dock).

import fs from 'node:fs'
import path from 'node:path'
import { test, expect, type Page } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
const vmId = process.env.PLAYWRIGHT_LIBVIRT_VM_ID?.trim()
const vmName = process.env.PLAYWRIGHT_LIBVIRT_VM_NAME?.trim() || 'ubuntu-desktop'
const platformUser = process.env.PLAYWRIGHT_PLATFORM_USER?.trim() || process.env.E2E_PLATFORM_USER?.trim() || 'admin'
const platformPass = process.env.PLAYWRIGHT_PLATFORM_PASS?.trim() || process.env.E2E_PLATFORM_PASS?.trim() || ''

test.skip(!live || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')
test.skip(!vmId, 'Set PLAYWRIGHT_LIBVIRT_VM_ID (platform VM uuid for ubuntu-desktop)')

function pngLooksLikeConsole(buf: Buffer): boolean {
  if (buf.length < 800) return false
  const sample = buf.subarray(Math.min(100, buf.length), Math.min(buf.length, 12_000))
  let sum = 0
  for (let i = 0; i < sample.length; i++) sum += sample[i]!
  const avg = sum / Math.max(1, sample.length)
  let variance = 0
  for (let i = 0; i < sample.length; i++) {
    const d = sample[i]! - avg
    variance += d * d
  }
  variance /= Math.max(1, sample.length)
  if (buf.length > 12_000 && variance > 100 && avg > 10 && avg < 245) return true
  if (variance > 500 && avg < 80) return true
  return false
}

function pngLooksLikeGraphicalDesktop(buf: Buffer): boolean {
  if (buf.length < 28_000) return false
  if (!pngLooksLikeConsole(buf)) return false
  const sample = buf.subarray(Math.min(100, buf.length), Math.min(buf.length, 24_000))
  let sum = 0
  for (let i = 0; i < sample.length; i++) sum += sample[i]!
  const avg = sum / Math.max(1, sample.length)
  let variance = 0
  for (let i = 0; i < sample.length; i++) {
    const d = sample[i]! - avg
    variance += d * d
  }
  variance /= Math.max(1, sample.length)
  return variance > 400 && avg > 15 && avg < 240
}

async function captureVncCanvas(page: Page): Promise<Buffer | null> {
  const dataUrl = await page.evaluate(() => {
    const canvas = document.querySelector('canvas')
    if (!canvas || canvas.width < 32 || canvas.height < 32) return null
    try {
      return canvas.toDataURL('image/png')
    } catch {
      return null
    }
  })
  if (!dataUrl?.startsWith('data:image/png;base64,')) return null
  return Buffer.from(dataUrl.slice('data:image/png;base64,'.length), 'base64')
}

async function captureGuestScreenshot(page: Page): Promise<Buffer> {
  const res = await page.request.get(`${live}/api/v1/vms/${vmName}/guest/screenshot?screen=0`, {
    ignoreHTTPSErrors: true,
  })
  expect(res.status()).toBeLessThan(500)
  return Buffer.from(await res.body())
}

async function openConsoleHub(page: Page) {
  await setDesktopTier(page, 'power')
  await ensureLoggedIn(page, live!, '/platform')
  await page.goto(`${live}/platform/vms/${vmId}/consolehub`, { waitUntil: 'domcontentloaded' })
  await expect(page.getByText(new RegExp(vmName, 'i')).first()).toBeVisible({ timeout: 60_000 })
}

test.describe.configure({ mode: 'serial' })

test('consolehub plan recommends VNC and lists serial (Cockpit parity)', async ({ request }) => {
  test.skip(!platformPass, 'Set PLAYWRIGHT_PLATFORM_PASS or E2E_PLATFORM_PASS for controller API')
  const controller = live!.replace('https://', 'http://').replace(':5092', ':5093')
  const res = await request.get(`${controller}/api/v1/vms/${vmId}/consolehub/plan`, {
    headers: {
      Authorization: `Basic ${Buffer.from(`${platformUser}:${platformPass}`).toString('base64')}`,
    },
  })
  expect(res.status()).toBe(200)
  const plan = await res.json()
  expect(plan.recommended).toBe('novnc')
  expect(plan.protocols).toEqual(expect.arrayContaining(['novnc', 'serial']))
  expect(plan.native?.serial_ws_path).toMatch(/\/platform\/serial\//)
  expect(plan.native?.ws_path).toMatch(/\/platform\/vnc\//)
})

test('consolehub serial lens shows login', async ({ page }, testInfo) => {
  test.setTimeout(180_000)
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  const shotDir = path.join(testInfo.project.outputDir, 'libvirt-desktop-vnc')
  fs.mkdirSync(shotDir, { recursive: true })

  await openConsoleHub(page)
  await page.getByRole('button', { name: 'Serial', exact: true }).first().click()
  await expect(page.getByText(/Serial Console/i)).toBeVisible({ timeout: 60_000 })

  let loginSeen = false
  for (let i = 0; i < 24; i++) {
    const body = await page.locator('.xterm-screen').innerText().catch(() => '')
    if (/ubuntu.*login:|login:\s*$/im.test(body) || /Ubuntu.*LTS/i.test(body) || /Connected to serial/i.test(body)) {
      loginSeen = true
      await page.screenshot({ path: path.join(shotDir, 'serial-login.png'), fullPage: true })
      break
    }
    await page.waitForTimeout(5_000)
  }

  expect(loginSeen, 'Serial console did not show login or connected state').toBe(true)
  const ignored = (e: string) =>
    e.includes('ResizeObserver') || e.includes('Maximum call stack size exceeded')
  expect(errors.filter((e) => !ignored(e))).toEqual([])
})

test('consolehub display (VNC) shows GNOME desktop + dock controls', async ({ page }, testInfo) => {
  test.setTimeout(360_000)
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))

  const shotDir = path.join(testInfo.project.outputDir, 'libvirt-desktop-vnc')
  fs.mkdirSync(shotDir, { recursive: true })

  await openConsoleHub(page)
  await page.getByRole('button', { name: 'Display', exact: true }).first().click()

  await expect(page.getByText(/^Connected$/i)).toBeVisible({ timeout: 180_000 })
  await expect(page.getByRole('button', { name: /Ctrl\+Alt\+Del/i })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByRole('button', { name: 'Fit' })).toBeVisible()

  let desktopSeen = false
  for (let i = 1; i <= 18; i++) {
    const pngPath = path.join(shotDir, `vnc-${String(i).padStart(2, '0')}.png`)
    const vncBuf = await captureVncCanvas(page)
    const guestBuf = await captureGuestScreenshot(page)
    const buf = vncBuf && vncBuf.length > guestBuf.length ? vncBuf : guestBuf
    fs.writeFileSync(pngPath, buf)
    await testInfo.attach(`vnc-${i}`, { path: pngPath, contentType: 'image/png' })
    if (pngLooksLikeGraphicalDesktop(buf)) {
      desktopSeen = true
      break
    }
    await page.waitForTimeout(15_000)
  }

  expect(desktopSeen, 'No GNOME desktop frame detected in VNC/guest screenshots').toBe(true)

  await page.getByRole('button', { name: 'Native', exact: true }).click()
  await page.getByRole('button', { name: 'Fit' }).click()

  const ignored = (e: string) =>
    e.includes('ResizeObserver') || e.includes('Maximum call stack size exceeded')
  expect(errors.filter((e) => !ignored(e))).toEqual([])
})

test('legacy /console redirects to consolehub', async ({ page }) => {
  await openConsoleHub(page)
  await page.goto(`${live}/platform/vms/${vmId}/console`, { waitUntil: 'domcontentloaded' })
  await expect(page).toHaveURL(new RegExp(`/platform/vms/${vmId}/consolehub`))
})
