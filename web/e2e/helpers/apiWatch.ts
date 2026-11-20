// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { Page, Response } from '@playwright/test'

export interface ApiFailure {
  url: string
  status: number
  reason: string
}

export interface ApiWarning {
  url: string
  status: number
  reason: string
}

function isApiUrl(url: string) {
  return url.includes('/api/v1/') || url.includes('/api/v1/platform/controller/')
}

export function createApiWatch(page: Page) {
  const failures: ApiFailure[] = []
  const warnings: ApiWarning[] = []
  const seen = new Set<string>()

  const handler = async (response: Response) => {
    const url = response.url()
    if (!isApiUrl(url)) return
    const status = response.status()
    const key = `${status}:${url}`
    if (seen.has(key)) return
    seen.add(key)

    if (status >= 500) {
      failures.push({ url, status, reason: 'HTTP 5xx' })
      return
    }

    const ct = (response.headers()['content-type'] ?? '').toLowerCase()
    if (status >= 200 && status < 300 && ct.includes('text/html') && url.includes('/api/v1/')) {
      failures.push({ url, status, reason: 'API returned HTML (SPA fallback?)' })
      return
    }

    if (status === 401 || status === 403 || status === 404) {
      warnings.push({ url, status, reason: 'optional endpoint denied or empty' })
    }
  }

  page.on('response', handler)

  return {
    reset() {
      failures.length = 0
      warnings.length = 0
      seen.clear()
    },
    getFailures: () => [...failures],
    getWarnings: () => [...warnings],
    dispose() {
      page.off('response', handler)
    },
  }
}
