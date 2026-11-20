// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import './playwright-node-env.js'
import { defineConfig, devices } from '@playwright/test'

const nodeOptions = [
  process.env.NODE_OPTIONS,
  '--disable-warning=DEP0205',
]
  .filter(Boolean)
  .join(' ')

export default defineConfig({
  testDir: './e2e',
  testIgnore: '**/live-ux-wiring.spec.ts',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 1,
  workers: process.env.CI ? 1 : undefined,
  timeout: 150_000,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: process.env.PLAYWRIGHT_LIVE_URL ?? 'http://127.0.0.1:5192',
    trace: 'on-first-retry',
    ignoreHTTPSErrors: Boolean(process.env.PLAYWRIGHT_LIVE_URL),
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: process.env.PLAYWRIGHT_LIVE_URL
    ? undefined
    : {
        command: 'npm run preview -- --host 127.0.0.1 --port 5192',
        url: 'http://127.0.0.1:5192',
        reuseExistingServer: !process.env.CI,
        timeout: 120_000,
        env: (() => {
          const env = { ...process.env, NODE_OPTIONS: nodeOptions }
          delete env.NO_COLOR
          return env
        })(),
      },
})
