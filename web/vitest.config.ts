// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    projects: [
      {
        // React component / hook tests — run in jsdom (requires browser globals)
        test: {
          name: 'jsdom',
          include: ['src/**/*.test.tsx'],
          environment: 'jsdom',
          environmentOptions: { jsdom: { url: 'http://localhost:3000' } },
        },
      },
      {
        // Pure utility / API client tests — run in Node (faster, no DOM needed)
        test: {
          name: 'node',
          include: ['src/**/*.test.ts'],
          environment: 'node',
        },
      },
    ],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      include: ['src/utils/**', 'src/api/**', 'src/hooks/**'],
      thresholds: {
        lines: 20,
        functions: 25,
      },
    },
  },
})
