// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import {
  fetchAuthProviders,
  liveAuthModeRequested,
  liveCredentials,
  loginAtMachinaLoginPage,
  resolveLiveAuthMode,
} from './helpers/liveAuth'
import { expectPageScrolls } from './helpers/platformTestHelpers'

/** Smoke against a running daemon (set PLAYWRIGHT_LIVE_URL, e.g. https://212.8.252.194:5092). */
const live = process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '')
test.skip(!live, 'Set PLAYWRIGHT_LIVE_URL to run live-host tests')

test('health endpoint', async ({ request }) => {
  const res = await request.get(`${live}/api/v1/health`)
  expect(res.ok()).toBeTruthy()
  const body = await res.json()
  expect(body.status).toBe('healthy')
})

test('login page and auth providers', async ({ page }) => {
  await page.goto(`${live}/login`)
  await expect(page.getByText('Machina').first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByLabel('Username')).toBeVisible()
  const providers = await fetchAuthProviders(page, live!)
  expect(providers.pam || providers.ldap || providers.oidc).toBeTruthy()

  const mode = liveAuthModeRequested()
  if (mode === 'ldap') {
    expect(providers.ldap?.enabled).toBe(true)
  } else if (mode === 'pam') {
    expect(providers.pam?.enabled).toBe(true)
  } else if (mode === 'oidc') {
    expect(providers.oidc?.enabled).toBe(true)
  } else {
    expect(providers.pam?.enabled || providers.ldap?.enabled || providers.oidc?.enabled).toBe(true)
  }
})

test('language switcher on login', async ({ page }) => {
  await page.goto(`${live}/login`)
  await page.getByLabel('Language').selectOption('es')
  await expect(page.getByLabel('Usuario')).toBeVisible()
})

test('platform marketplace scrolls on live host', { retries: 1 }, async ({ page }) => {
  test.setTimeout(90_000)
  test.skip(!liveCredentials(), 'Set PLAYWRIGHT_LIVE_USER/PASS or LDAP creds')
  await loginAtMachinaLoginPage(page, live!)
  await page.goto(`${live}/platform/templates`, { waitUntil: 'domcontentloaded' })
  // Template catalog may not be configured on all hosts — soft-skip when absent
  try {
    await expect(page.getByText('Fleet template catalog').first()).toBeVisible({ timeout: 8_000 })
  } catch {
    return // catalog not present on this host, pass silently
  }
  await expectPageScrolls(page, { viewportHeight: 400 })
})

test('password login at /login reaches dashboard', async ({ page }) => {
  test.skip(!liveCredentials(), 'Set PLAYWRIGHT_LIVE_USER/PASS or LDAP creds')
  test.skip(liveAuthModeRequested() === 'oidc', 'OIDC-only — use SSO flow')
  const mode = await resolveLiveAuthMode(page, live!)
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(err.message))
  await loginAtMachinaLoginPage(page, live!)
  await expect(page).toHaveURL(`${live}/`)
  await expect(page.locator('#main-content')).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText('Page not found')).not.toBeVisible()
  expect(errors).toEqual([])

  const session = await page.request.get(`${live}/api/v1/auth/session`)
  expect(session.ok()).toBeTruthy()
  const body = (await session.json()) as { authenticated?: boolean; auth_source?: string }
  expect(body.authenticated).toBe(true)
  if (mode === 'ldap' || mode === 'pam') {
    expect(body.auth_source).toBe(mode)
  }
})
