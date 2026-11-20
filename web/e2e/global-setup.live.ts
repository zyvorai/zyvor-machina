// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium, type FullConfig } from '@playwright/test'
import { liveCredentials, loginAtMachinaLoginPage, setDesktopTier } from './helpers/liveAuth'

const AUTH_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '.auth')
const STORAGE_PATH = path.join(AUTH_DIR, 'live-user.json')

export default async function globalSetup(_config: FullConfig) {
  const baseUrl = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
  const creds = liveCredentials()
  if (!baseUrl || !creds) return

  fs.mkdirSync(AUTH_DIR, { recursive: true })

  const browser = await chromium.launch()
  const context = await browser.newContext({ ignoreHTTPSErrors: true })
  const page = await context.newPage()
  await setDesktopTier(page, 'power')
  await loginAtMachinaLoginPage(page, baseUrl)
  await context.storageState({ path: STORAGE_PATH })
  await browser.close()
}

export { STORAGE_PATH }
