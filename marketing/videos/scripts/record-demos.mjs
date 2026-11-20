#!/usr/bin/env node
// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Record Machina customer demo clips against a live host.
 *
 *   PLAYWRIGHT_LIVE_URL=https://80.79.5.173:5092 \
 *   PLAYWRIGHT_LIVE_USER=sus PLAYWRIGHT_LIVE_PASS=max \
 *   node marketing/videos/scripts/record-demos.mjs [01|02|...|all]
 */
import { createRequire } from 'node:module'
import { mkdirSync, readFileSync, renameSync, readdirSync, existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const REPO = resolve(ROOT, '../..')
const require = createRequire(join(REPO, 'web/package.json'))
const { chromium } = require('playwright')
const RAW = join(ROOT, 'out', 'raw')
const manifest = JSON.parse(readFileSync(join(ROOT, 'clips.json'), 'utf8'))

const BASE = (process.env.PLAYWRIGHT_LIVE_URL || manifest.baseUrl).replace(/\/$/, '')
const USER = process.env.PLAYWRIGHT_LIVE_USER || process.env.MACHINA_USER || 'sus'
const PASS = process.env.PLAYWRIGHT_LIVE_PASS || process.env.MACHINA_PASS || 'max'
const DESKTOP = manifest.vms.desktop
const SERVER = manifest.vms.server

const want = process.argv[2] || 'all'
const selected =
  want === 'all' ? manifest.clips : manifest.clips.filter((c) => c.id === want || c.slug === want)

if (!selected.length) {
  console.error(`Unknown clip: ${want}`)
  process.exit(1)
}

mkdirSync(RAW, { recursive: true })

async function login(page) {
  await page.goto(`${BASE}/login`, { waitUntil: 'domcontentloaded', timeout: 60_000 })
  await page.waitForTimeout(800)
  if (await page.locator('#login-username').isVisible().catch(() => false)) {
    await page.locator('#login-username').fill(USER)
    await page.locator('#login-password').fill(PASS)
    await page.getByRole('button', { name: /sign in/i }).first().click()
    await page.waitForURL((u) => !u.pathname.includes('/login'), { timeout: 45_000 }).catch(() => {})
  }
  await page.evaluate(() => {
    localStorage.setItem('machina-platform-desktop-tier', 'power')
    localStorage.setItem('zyvor-platform-welcome-done', '1')
  })
}

async function settle(page, ms = 1500) {
  await page.waitForTimeout(ms)
}

async function moveAround(page) {
  await page.mouse.move(200, 200)
  await page.waitForTimeout(400)
  await page.mouse.move(900, 500)
  await page.waitForTimeout(400)
  await page.mouse.move(640, 360)
}

async function canvasIsLive(page) {
  // Prefer page screenshot — getImageData on the VNC canvas is often tainted/black
  // even when the guest is visibly painting.
  try {
    const buf = await page.screenshot({ type: 'png', clip: { x: 480, y: 200, width: 960, height: 540 } })
    // PNG IHDR + IDAT; rough brightness via raw decode is heavy — use sharp-less heuristic:
    // count non-near-black pixels by sampling PNG via Playwright evaluate on an ImageBitmap... 
    // Fallback: write is expensive; use evaluate draw of canvas to a clean 2d canvas copy.
  } catch {
    /* continue to canvas probe */
  }
  return page.evaluate(() => {
    const canvas = document.querySelector('canvas')
    if (!canvas || canvas.width < 64 || canvas.height < 64) return false
    // Copy via drawImage into a same-origin canvas so getImageData works even if
    // the RFB canvas is considered tainted in some browsers.
    try {
      const off = document.createElement('canvas')
      off.width = Math.min(canvas.width, 320)
      off.height = Math.min(canvas.height, 240)
      const octx = off.getContext('2d')
      if (!octx) return false
      octx.drawImage(canvas, 0, 0, off.width, off.height)
      const sample = octx.getImageData(0, 0, off.width, off.height).data
      let sum = 0
      let bright = 0
      const n = sample.length / 4
      for (let j = 0; j < sample.length; j += 4) {
        const v = (sample[j] + sample[j + 1] + sample[j + 2]) / 3
        sum += v
        if (v > 18) bright++
      }
      const avg = sum / n
      return avg > 14 || bright / n > 0.08
    } catch {
      return canvas.width > 100 && canvas.height > 100
    }
  })
}

/** Screenshot-based live check (authoritative for recording QA). */
async function frameLooksLive(page) {
  const { writeFileSync } = await import('node:fs')
  const { join } = await import('node:path')
  const tmp = join('/tmp', `machina-frame-${Date.now()}.png`)
  await page.screenshot({ path: tmp, type: 'png', clip: { x: 400, y: 160, width: 1120, height: 700 } })
  // Inline brightness without Pillow dependency in the browser path — use a tiny spawn
  const { execFileSync } = await import('node:child_process')
  try {
    const out = execFileSync(
      'python3',
      [
        '-c',
        `from PIL import Image; im=Image.open(${JSON.stringify(tmp)}).convert('RGB'); px=list(im.getdata()); avg=sum(sum(p) for p in px)/(len(px)*3); print(avg)`,
      ],
      { encoding: 'utf8' },
    )
    const avg = parseFloat(out.trim())
    console.log(`  frame brightness=${avg.toFixed(1)}`)
    return avg > 18
  } catch {
    return false
  }
}

async function clickVncCanvas(page) {
  const canvas = page.locator('canvas').first()
  if (await canvas.count()) {
    const box = await canvas.boundingBox()
    if (box) {
      const x = box.x + box.width * 0.5
      const y = box.y + box.height * 0.5
      await page.mouse.move(x, y)
      await page.mouse.click(x, y)
      await page.waitForTimeout(300)
      await page.mouse.click(x + 40, y + 30)
      await page.waitForTimeout(300)
      // Nudge guest to redraw
      await page.keyboard.press('Control')
      return
    }
  }
  await page.mouse.click(960, 540)
}

/** Wait until Cinema VNC is visibly painting (not black), then linger for the shot. */
async function waitCinema(page, { lingerMs = 8000 } = {}) {
  await page.getByTestId('cinema-shell').waitFor({ state: 'visible', timeout: 90_000 }).catch(() => {})
  await page.getByText(/^Connected$/i).first().waitFor({ state: 'visible', timeout: 180_000 }).catch(() => {})
  await settle(page, 2000)

  // Prefer Fit so the guest desktop fills the frame
  await page.getByText(/^Fit$/i).first().click({ timeout: 5000 }).catch(() => {})
  await settle(page, 1000)

  let live = false
  for (let i = 0; i < 12; i++) {
    await clickVncCanvas(page)
    // Keyboard nudge helps Windows guests wake the display
    await page.keyboard.press('Escape').catch(() => {})
    await settle(page, 800)
    live = (await canvasIsLive(page)) || (await frameLooksLive(page))
    if (live) {
      console.log(`  VNC canvas live after ${i + 1} click(s)`)
      break
    }
    await settle(page, 1200)
  }
  if (!live) {
    console.warn('  WARNING: VNC canvas still looks blank — filming anyway')
  }

  // Hold on the live desktop
  await settle(page, Math.min(lingerMs, 4000))
  await moveAround(page)
  await settle(page, 2000)
  // Idle-hide HUD for cinematic beat, then reveal
  await page.mouse.move(8, 8)
  await settle(page, 3500)
  await clickVncCanvas(page)
  await settle(page, Math.max(2000, lingerMs - 5500))
}

async function recordClip(clip) {
  const outName = `${clip.id}-${clip.slug}`
  const sessionDir = join(RAW, `_session_${outName}`)
  mkdirSync(sessionDir, { recursive: true })

  console.log(`\n▶ Recording ${outName} …`)
  const browser = await chromium.launch({
    // headed=false is fine for noVNC if we click the canvas; keep GPU enabled
    headless: true,
    args: ['--ignore-certificate-errors', '--use-gl=angle', '--enable-webgl'],
  })
  const context = await browser.newContext({
    ignoreHTTPSErrors: true,
    viewport: { width: 1920, height: 1080 },
    recordVideo: { dir: sessionDir, size: { width: 1920, height: 1080 } },
  })
  const page = await context.newPage()
  page.setDefaultTimeout(45_000)

  try {
    await login(page)
    await settle(page, 1000)

    switch (clip.id) {
      case '01':
        await page.goto(
          `${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=cinema&protocol=novnc`,
          { waitUntil: 'domcontentloaded' },
        )
        await waitCinema(page, { lingerMs: 12000 })
        await page.getByText(/^Fill$/i).first().click({ timeout: 5000 }).catch(() => {})
        await settle(page, 2000)
        await clickVncCanvas(page)
        await settle(page, 2500)
        await page.getByText(/^Fit$/i).first().click({ timeout: 5000 }).catch(() => {})
        await settle(page, 1500)
        await clickVncCanvas(page)
        await settle(page, 3000)
        break

      case '02':
        await page.goto(`${BASE}/platform/mission-control/live`, { waitUntil: 'domcontentloaded' })
        await settle(page, 4000)
        await moveAround(page)
        await settle(page, 3000)
        // Click first live tile / cinema link
        {
          const link = page.locator(`a[href*="/consolehub"]`).first()
          if (await link.count()) {
            await link.click()
            await waitCinema(page)
          } else {
            await page.goto(`${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=cinema`, {
              waitUntil: 'domcontentloaded',
            })
            await waitCinema(page)
          }
        }
        break

      case '03':
        await page.goto(`${BASE}/platform/vms`, { waitUntil: 'domcontentloaded' })
        await settle(page, 2500)
        await page.getByRole('button', { name: /^Gallery$/i }).click({ timeout: 8000 }).catch(() => {})
        await settle(page, 3000)
        await moveAround(page)
        await settle(page, 2000)
        {
          const cinema = page
            .locator(`a[href*="${DESKTOP.id}/consolehub"], a[aria-label*="Cinema" i]`)
            .first()
          if (await cinema.count()) {
            await cinema.click()
            await waitCinema(page)
          } else {
            await page.goto(`${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=cinema`, {
              waitUntil: 'domcontentloaded',
            })
            await waitCinema(page)
          }
        }
        break

      case '04':
        await page.goto(
          `${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=cinema&protocol=novnc`,
          { waitUntil: 'domcontentloaded' },
        )
        await waitCinema(page, { lingerMs: 6000 })
        await page
          .getByRole('link', { name: /Studio/i })
          .first()
          .click({ timeout: 8000 })
          .catch(async () => {
            await page.goto(
              `${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=studio&protocol=novnc`,
              { waitUntil: 'domcontentloaded' },
            )
          })
        await settle(page, 3500)
        await clickVncCanvas(page)
        await settle(page, 2500)
        await page.getByRole('button', { name: /Serial/i }).first().click({ timeout: 5000 }).catch(() => {})
        await settle(page, 2000)
        await page.getByRole('button', { name: /Display/i }).first().click({ timeout: 5000 }).catch(() => {})
        await settle(page, 1500)
        await clickVncCanvas(page)
        await settle(page, 2500)
        await page.getByTestId('ops-shelf-handle').click({ timeout: 5000 }).catch(() => {})
        await settle(page, 3500)
        break

      case '05':
        // Show VM detail on the desktop guest so Cinema has a real screen
        await page.goto(`${BASE}/platform/vms/${DESKTOP.id}`, { waitUntil: 'domcontentloaded' })
        await settle(page, 3000)
        await moveAround(page)
        await page.getByRole('tab', { name: /Access|Overview/i }).first().click({ timeout: 5000 }).catch(() => {})
        await settle(page, 2500)
        await page
          .getByRole('link', { name: /Open Cinema|Cinema/i })
          .first()
          .click({ timeout: 8000 })
          .catch(async () => {
            await page.goto(
              `${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=cinema&protocol=novnc`,
              { waitUntil: 'domcontentloaded' },
            )
          })
        await waitCinema(page, { lingerMs: 10000 })
        break

      case '06':
        await page.goto(`${BASE}/platform/vms/${SERVER.id}?tab=disks`, { waitUntil: 'domcontentloaded' })
        await settle(page, 2000)
        // Snapshots often under More / Disks / dedicated tab
        await page.getByRole('tab', { name: /Snapshot/i }).click({ timeout: 5000 }).catch(() => {})
        await page.getByRole('button', { name: /^More$/i }).click({ timeout: 4000 }).catch(() => {})
        await page.getByRole('menuitem', { name: /Snapshot/i }).click({ timeout: 4000 }).catch(() => {})
        await settle(page, 1500)
        {
          const snapBtn = page.getByRole('button', { name: /Create snapshot|New snapshot|Snapshot/i }).first()
          if (await snapBtn.count()) {
            await snapBtn.click()
            await settle(page, 1000)
            const nameBox = page.getByRole('textbox').last()
            if (await nameBox.isVisible().catch(() => false)) {
              await nameBox.fill(`demo-snap-${Date.now().toString(36).slice(-4)}`)
            }
            await page.getByRole('button', { name: /^Create$|^Save$|Create snapshot/i }).last().click({ timeout: 8000 }).catch(() => {})
            await settle(page, 6000)
          } else {
            // Fallback: Hardware / Power menu snapshot
            await page.getByRole('button', { name: /Power & more/i }).click({ timeout: 5000 }).catch(() => {})
            await settle(page, 4000)
          }
        }
        break

      case '07':
        await page.goto(`${BASE}/platform/vms/${SERVER.id}?tab=access`, { waitUntil: 'domcontentloaded' })
        await settle(page, 2500)
        await page.getByRole('tab', { name: /Access/i }).click({ timeout: 5000 }).catch(() => {})
        await settle(page, 2000)
        await page.getByRole('button', { name: /Expose SSH|SSH/i }).first().click({ timeout: 8000 }).catch(() => {})
        await settle(page, 4000)
        await moveAround(page)
        await settle(page, 3000)
        break

      case '08':
        await page.goto(`${BASE}/platform/vms/${SERVER.id}`, { waitUntil: 'domcontentloaded' })
        await settle(page, 2000)
        await page.getByRole('button', { name: /Ask Zeus|Open Spotlight/i }).first().click({ timeout: 8000 }).catch(() => {})
        await settle(page, 2500)
        {
          const search = page.getByRole('searchbox').first()
          if (await search.isVisible().catch(() => false)) {
            await search.fill('diagnose this VM')
            await settle(page, 2500)
            await page.keyboard.press('Enter').catch(() => {})
            await settle(page, 4000)
          }
        }
        await page.keyboard.press('Escape').catch(() => {})
        await settle(page, 1500)
        break

      case '09':
        await page.goto(`${BASE}/platform/network-canvas`, { waitUntil: 'domcontentloaded' })
        await settle(page, 4000)
        await page.mouse.move(400, 300)
        await page.mouse.down()
        await page.mouse.move(800, 500, { steps: 20 })
        await page.mouse.up()
        await settle(page, 2000)
        await page.mouse.wheel(0, -300)
        await settle(page, 2000)
        await page.mouse.wheel(0, 400)
        await settle(page, 2500)
        await moveAround(page)
        await settle(page, 2000)
        break

      case '10': {
        const name = `demo-cinema-${Date.now().toString(36).slice(-5)}`
        await page.goto(`${BASE}/platform/vms`, { waitUntil: 'domcontentloaded' })
        await settle(page, 2500)
        await page.getByTestId('machine-finder-new-vm').click()
        await settle(page, 2000)
        const nameBox = page.getByRole('textbox', { name: /Virtual machine name|name/i })
        await nameBox.click()
        await nameBox.fill('')
        await nameBox.fill(name)
        await settle(page, 2000)
        await page.getByRole('button', { name: /^Next$/i }).click()
        await settle(page, 2500)
        await page.getByRole('button', { name: /Ubuntu 24\.04 LTS(?! GNOME)/i }).first().click()
        await settle(page, 2000)
        await page.getByRole('button', { name: /^Next$/i }).click()
        await settle(page, 2500)
        await page.getByRole('radio', { name: /Small/i }).click()
        await settle(page, 2000)
        await page.getByRole('button', { name: /^Next$/i }).click()
        await settle(page, 3000)
        await moveAround(page)
        await settle(page, 2000)
        await page.getByRole('button', { name: /Create VM/i }).click()
        await settle(page, 5000)
        // Disk convert can take minutes — cut to a live desktop Cinema for the wow payoff
        await page.goto(
          `${BASE}/platform/vms/${DESKTOP.id}/consolehub?mode=cinema&protocol=novnc`,
          { waitUntil: 'domcontentloaded' },
        )
        await waitCinema(page, { lingerMs: 12000 })
        break
      }

      default:
        throw new Error(`No scenario for ${clip.id}`)
    }

    await settle(page, 1500)
  } finally {
    await context.close()
    await browser.close()
  }

  // Playwright writes a random webm name in sessionDir — move to stable path
  const files = readdirSync(sessionDir).filter((f) => f.endsWith('.webm'))
  if (!files.length) throw new Error(`No video recorded for ${outName}`)
  const dest = join(RAW, `${outName}.webm`)
  renameSync(join(sessionDir, files[0]), dest)
  console.log(`✓ ${dest}`)
  return dest
}

for (const clip of selected) {
  try {
    await recordClip(clip)
  } catch (err) {
    console.error(`✗ ${clip.id}-${clip.slug}:`, err.message || err)
  }
}

console.log('\nDone. Raw clips in', RAW)
