// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import './playwright-node-env.js'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig, devices } from '@playwright/test'

const AUTH_STATE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'e2e/.auth/live-user.json')

export default defineConfig({
  testDir: './e2e',
  testMatch: 'live-ux-wiring.spec.ts',
  globalSetup: './e2e/global-setup.live.ts',
  workers: 1,
  timeout: 180_000,
  retries: 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: process.env.PLAYWRIGHT_LIVE_URL,
    ignoreHTTPSErrors: true,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    storageState: AUTH_STATE,
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
