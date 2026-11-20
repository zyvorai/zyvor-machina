// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './helpers/liveAuth'

const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
const vmId = process.env.PLAYWRIGHT_LIBVIRT_VM_ID?.trim()

test.skip(!live || !liveCredentials() || !vmId)

test('fit canvas debug', async ({ page }) => {
  test.setTimeout(120_000)
  await setDesktopTier(page, 'power')
  await ensureLoggedIn(page, live!, '/platform')
  await page.goto(`${live}/platform/vms/${vmId}/consolehub`)
  await page.waitForSelector('text=Connected', { timeout: 120_000 })
  for (let i = 0; i < 30; i++) {
    const ready = await page.evaluate(() => {
      const canvas = document.querySelector('canvas')
      return Boolean(canvas && canvas.width > 64 && canvas.height > 64)
    })
    if (ready) break
    await page.waitForTimeout(2000)
  }
  const info = await page.evaluate(() => {
    const canvas = document.querySelector('canvas')
    const container = canvas?.closest('[style*="transform"]') ?? canvas?.parentElement
    const ctx = canvas?.getContext('2d')
    let avg = 0
    if (ctx && canvas && canvas.width > 0 && canvas.height > 0) {
      const data = ctx.getImageData(0, 0, Math.min(200, canvas.width), Math.min(200, canvas.height)).data
      let sum = 0
      for (let i = 0; i < data.length; i += 4) sum += data[i]! + data[i + 1]! + data[i + 2]!
      avg = sum / Math.max(1, data.length / 4) / 3
    }
    return {
      canvas: canvas ? { w: canvas.width, h: canvas.height, cssW: canvas.getBoundingClientRect().width, avg } : null,
      containerTransform: container ? getComputedStyle(container as Element).transform : null,
    }
  })
  console.log(JSON.stringify(info))
  await page.screenshot({ path: 'test-results/fit-debug2.png', fullPage: true })
})
