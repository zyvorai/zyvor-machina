// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Capture the README screenshots (docs/ux/machina-*.png) from a live deployment.
// Uses the system Google Chrome, so no Playwright browser download is needed.
//
//   MACHINA_BASE_URL=https://HOST:5092 MACHINA_USER=sus MACHINA_PASS=... \
//     node web/scripts/capture-readme-screens.mjs [name ...]
import { chromium } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const base = (process.env.MACHINA_BASE_URL || 'https://127.0.0.1:5092').replace(/\/$/, '')
const user = process.env.MACHINA_USER || ''
const pass = process.env.MACHINA_PASS || ''
const vm = process.env.MACHINA_VM || 'web-frontend-01'
const out = join(dirname(fileURLToPath(import.meta.url)), '../../docs/ux')

const shots = [
  { name: 'machina-dashboard', path: '/' },
  { name: 'machina-vms', path: '/vms' },
  { name: 'machina-vm-detail', path: `/vms/${vm}`, settle: 6000 },
  { name: 'machina-fleet', path: '/platform/ha' },
  { name: 'machina-fleet-cloud', path: '/fleet-cloud' },
  { name: 'machina-zyra', path: '/platform/zyra' },
]

const only = process.argv.slice(2)
mkdirSync(out, { recursive: true })

const browser = await chromium.launch({ channel: 'chrome', headless: true })
const context = await browser.newContext({
  ignoreHTTPSErrors: true,
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
  colorScheme: 'light',
})
const page = await context.newPage()

const login = await page.request.post(`${base}/api/v1/auth/login`, { data: { username: user, password: pass } })
if (!login.ok()) {
  console.error(`login failed: ${login.status()} ${await login.text()}`)
  process.exit(1)
}

for (const s of shots) {
  if (only.length && !only.includes(s.name)) continue
  // Live WebSocket updates keep the network busy, so "networkidle" never fires.
  await page.goto(`${base}${s.path}`, { waitUntil: 'load', timeout: 60000 })
  await page.waitForTimeout(s.settle ?? 4000)
  const file = join(out, `${s.name}.png`)
  await page.screenshot({ path: file })
  console.log(`wrote docs/ux/${s.name}.png  (${s.path})`)
}

await browser.close()
