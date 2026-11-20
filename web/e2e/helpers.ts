// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { Page } from '@playwright/test'

/** Wait until platform session + info mocks have responded (shell bridge depends on info). */
export async function waitForPlatformSession(page: Page) {
  await page.waitForResponse(
    (r) => {
      const url = r.url()
      return url.includes('/api/v1/') && (
        url.includes('/auth/session')
        || url.includes('/system/platform-info')
        || url.includes('/auth/providers')
      )
    },
    { timeout: 20_000 },
  ).catch(() => { /* already fulfilled */ })
}
