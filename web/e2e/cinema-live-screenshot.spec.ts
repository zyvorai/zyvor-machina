// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// One-shot live verification — Cinema mode visibility + screenshot.

import fs from 'node:fs'
import path from 'node:path'
import { test, expect } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
const vmId = process.env.PLAYWRIGHT_LIBVIRT_VM_ID?.trim()
const vmName = process.env.PLAYWRIGHT_LIBVIRT_VM_NAME?.trim() || 'ubuntu-desktop'

test.skip(!live || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')
test.skip(!vmId, 'Set PLAYWRIGHT_LIBVIRT_VM_ID')

test('Cinema mode shows shell, controls, and VNC desktop', async ({ page }, testInfo) => {
  test.setTimeout(360_000)
  const outDir = path.join(testInfo.project.outputDir, 'cinema-live')
  fs.mkdirSync(outDir, { recursive: true })

  await setDesktopTier(page, 'power')
  await ensureLoggedIn(page, live!, '/platform')
  await page.goto(`${live}/platform/vms/${vmId}/consolehub`, { waitUntil: 'domcontentloaded' })

  await expect(page.getByTestId('cinema-shell')).toBeVisible({ timeout: 60_000 })
  await expect(page.getByText('Machina Cinema')).toBeVisible()
  await expect(page.getByText(new RegExp(vmName, 'i')).first()).toBeVisible()
  await expect(page.getByTestId('cinema-control-strip')).toBeVisible({ timeout: 30_000 })
  await expect(page.getByTestId('ops-shelf-handle')).toBeVisible()
  await expect(page.getByTestId('cinema-offline-overlay')).toHaveCount(0)

  await expect(page.getByText(/^Connected$/i)).toBeVisible({ timeout: 180_000 })
  await expect(page.getByText(/Fit/i).first()).toBeVisible()

  let canvasNonBlack = false
  for (let i = 0; i < 24; i++) {
    canvasNonBlack = await page.evaluate(() => {
      const canvas = document.querySelector('canvas')
      if (!canvas || canvas.width < 64 || canvas.height < 64) return false
      const ctx = canvas.getContext('2d')
      if (!ctx) return false
      const sample = ctx.getImageData(0, 0, Math.min(canvas.width, 320), Math.min(canvas.height, 240)).data
      let sum = 0
      for (let j = 0; j < sample.length; j += 4) sum += sample[j]! + sample[j + 1]! + sample[j + 2]!
      const avg = sum / Math.max(1, (sample.length / 4) * 3)
      return avg > 12 && avg < 245
    })
    if (canvasNonBlack) break
    await page.waitForTimeout(2_000)
  }
  expect(canvasNonBlack, 'Fit mode canvas stayed blank on first open').toBe(true)

  await page.mouse.move(640, 400)
  await expect(page.getByTestId('cinema-control-strip')).toHaveAttribute('data-idle', 'false')

  const shotPath = path.join(outDir, 'cinema-mode.png')
  await page.screenshot({ path: shotPath, fullPage: true })
  await testInfo.attach('cinema-mode', { path: shotPath, contentType: 'image/png' })

  const canvasShot = path.join(outDir, 'cinema-vnc-canvas.png')
  const dataUrl = await page.evaluate(() => {
    const canvas = document.querySelector('canvas')
    if (!canvas) return null
    try {
      return canvas.toDataURL('image/png')
    } catch {
      return null
    }
  })
  expect(dataUrl?.startsWith('data:image/png;base64,')).toBeTruthy()
  fs.writeFileSync(canvasShot, Buffer.from(dataUrl!.slice('data:image/png;base64,'.length), 'base64'))
  await testInfo.attach('cinema-vnc-canvas', { path: canvasShot, contentType: 'image/png' })
})
