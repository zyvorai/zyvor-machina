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
  testMatch: 'platform-feature-matrix.spec.ts',
  workers: 1,
  timeout: 60_000,
  retries: 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: 'http://127.0.0.1:5192',
    trace: 'on-first-retry',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
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
