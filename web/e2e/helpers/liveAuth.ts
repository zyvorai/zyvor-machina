// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { expect, type Page } from '@playwright/test'

export type DesktopTier = 'normal' | 'power' | 'advanced'
export type LiveAuthMode = 'pam' | 'ldap' | 'oidc' | 'auto'

const TIER_KEY = 'machina-platform-desktop-tier'
const WELCOME_KEY = 'zyvor-platform-welcome-done'

/** Primary Machina PAM submit label (OIDC off). Secondary when SSO is enabled. */
export const MACHINA_LOGIN_SUBMIT =
  /sign in to machina|sign in with password|sign in|log in/i

export interface AuthProviders {
  pam?: { enabled?: boolean }
  ldap?: { enabled?: boolean }
  oidc?: { enabled?: boolean; button_label?: string }
}

export function liveAuthModeRequested(): LiveAuthMode {
  const raw = (process.env.PLAYWRIGHT_LIVE_AUTH || process.env.E2E_AUTH_MODE || 'auto').toLowerCase()
  if (raw === 'pam' || raw === 'ldap' || raw === 'oidc') return raw
  return 'auto'
}

export function liveCredentials() {
  const mode = liveAuthModeRequested()
  if (mode === 'oidc') return null
  if (mode === 'ldap') {
    const user =
      process.env.PLAYWRIGHT_LIVE_LDAP_USER ||
      process.env.E2E_LDAP_USER ||
      process.env.PLAYWRIGHT_LIVE_USER
    const pass =
      process.env.PLAYWRIGHT_LIVE_LDAP_PASS ||
      process.env.E2E_LDAP_PASS ||
      process.env.PLAYWRIGHT_LIVE_PASS
    return user && pass ? { user, pass } : null
  }
  const user = process.env.PLAYWRIGHT_LIVE_USER || process.env.E2E_USER
  const pass = process.env.PLAYWRIGHT_LIVE_PASS || process.env.E2E_PASSWORD
  return user && pass ? { user, pass } : null
}

export async function fetchAuthProviders(page: Page, baseUrl: string): Promise<AuthProviders> {
  const res = await page.request.get(`${baseUrl}/api/v1/auth/providers`, {
    ignoreHTTPSErrors: true,
  })
  if (!res.ok()) return {}
  return (await res.json()) as AuthProviders
}

export async function resolveLiveAuthMode(page: Page, baseUrl: string): Promise<Exclude<LiveAuthMode, 'auto'>> {
  const requested = liveAuthModeRequested()
  if (requested !== 'auto') return requested
  const providers = await fetchAuthProviders(page, baseUrl)
  if (providers.ldap?.enabled) {
    const creds = liveCredentials()
    if (
      process.env.PLAYWRIGHT_LIVE_LDAP_USER ||
      process.env.E2E_LDAP_USER ||
      creds?.user.includes('@')
    ) {
      return 'ldap'
    }
    if (providers.pam?.enabled) return 'pam'
    return 'ldap'
  }
  if (providers.pam?.enabled) return 'pam'
  if (providers.oidc?.enabled) return 'oidc'
  return 'pam'
}

const USERNAME = '#login-username, #username'
const PASSWORD = '#login-password, #password'

export async function fillMachinaLoginForm(page: Page, user: string, pass: string) {
  const username = page.locator(USERNAME)
  if (await username.isVisible().catch(() => false)) await username.fill(user)
  const password = page.locator(PASSWORD)
  if (!(await password.isVisible().catch(() => false))) {
    await page.getByRole('button', { name: 'Continue' }).click()
  }
  await password.fill(pass)
}

export async function submitMachinaLogin(page: Page) {
  const btn = page.getByRole('button', { name: MACHINA_LOGIN_SUBMIT })
  await expect(btn).toBeEnabled({ timeout: 10_000 })
  await btn.click()
}

export async function isMachinaLoginVisible(page: Page) {
  return page.locator(USERNAME).isVisible().catch(() => false)
}

async function hasValidSession(page: Page, baseUrl: string): Promise<boolean> {
  try {
    const res = await page.request.get(`${baseUrl}/api/v1/auth/session`, {
      ignoreHTTPSErrors: true,
    })
    if (!res.ok()) return false
    const body = (await res.json()) as { authenticated?: boolean }
    return body.authenticated === true
  } catch {
    return false
  }
}

export interface EnsureLoggedInOptions {
  tier?: DesktopTier
  /** When false, only establish session/tier — caller navigates to the route under test. */
  navigate?: boolean
  entryPath?: string
}

/** Sign in via the Machina login form when credentials are set. Reuses existing cookie session when valid. */
export async function ensureLoggedIn(
  page: Page,
  baseUrl: string,
  entryPathOrOptions: string | EnsureLoggedInOptions = '/platform',
  legacyTier: DesktopTier = 'power',
) {
  const opts: EnsureLoggedInOptions =
    typeof entryPathOrOptions === 'string'
      ? { entryPath: entryPathOrOptions, tier: legacyTier, navigate: true }
      : { entryPath: '/platform', tier: 'power', navigate: false, ...entryPathOrOptions }
  const { entryPath = '/platform', tier = 'power', navigate = true } = opts

  const creds = liveCredentials()
  await setDesktopTier(page, tier)
  if (creds) {
    const reuse = process.env.PLAYWRIGHT_LIVE_REUSE_AUTH !== '0'
    if (!reuse || !(await hasValidSession(page, baseUrl))) {
      try {
        await loginAtMachinaLoginPage(page, baseUrl)
      } catch {
        // Already authenticated or /login redirected away — continue to entry path.
      }
    }
  }
  if (!navigate) return

  const target = entryPath === '/' ? '/platform' : entryPath
  await page.goto(`${baseUrl}${target}`, { waitUntil: 'domcontentloaded', timeout: 180_000 })
  await page.locator(USERNAME).waitFor({ state: 'hidden', timeout: 20_000 }).catch(() => {})
}

/** Password login starting at `/login`; expects redirect to dashboard (`/`). */
export async function loginAtMachinaLoginPage(page: Page, baseUrl: string) {
  const mode = await resolveLiveAuthMode(page, baseUrl)
  if (mode === 'oidc') {
    throw new Error('OIDC-only host — set PLAYWRIGHT_LIVE_AUTH=pam|ldap or use SSO manually')
  }
  const creds = liveCredentials()
  if (!creds) {
    throw new Error(
      mode === 'ldap'
        ? 'Set PLAYWRIGHT_LIVE_LDAP_USER/PASS or E2E_LDAP_USER/PASS for LDAP login'
        : 'Set PLAYWRIGHT_LIVE_USER and PLAYWRIGHT_LIVE_PASS',
    )
  }
  await page.goto(`${baseUrl}/login`, { waitUntil: 'domcontentloaded' })
  await page.locator(`${USERNAME}, ${PASSWORD}`).first().waitFor({ state: 'visible', timeout: 15_000 })
  await fillMachinaLoginForm(page, creds.user, creds.pass)
  await submitMachinaLogin(page)
  await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 30_000 })
}

export async function logoutIfAuthenticated(page: Page, baseUrl: string) {
  try {
    if (await hasValidSession(page, baseUrl)) {
      await page.request.post(`${baseUrl}/api/v1/auth/logout`, { ignoreHTTPSErrors: true })
    }
  } catch {
    /* best-effort */
  }
}

export async function setDesktopTier(page: Page, tier: DesktopTier) {
  await page.addInitScript(
    ([t, welcomeKey, tierKey]) => {
      localStorage.setItem(welcomeKey, '1')
      localStorage.setItem(tierKey, t)
    },
    [tier, WELCOME_KEY, TIER_KEY] as const,
  )
}

export interface PlatformInfoFlags {
  k8sEnabled: boolean
}

export async function fetchPlatformFlags(page: Page, baseUrl: string): Promise<PlatformInfoFlags> {
  try {
    const res = await page.request.get(`${baseUrl}/api/v1/system/platform-info`, {
      ignoreHTTPSErrors: true,
    })
    if (!res.ok()) return { k8sEnabled: false }
    const body = (await res.json()) as {
      kubevirt?: { enabled?: boolean; exec_enabled?: boolean }
    }
    return {
      k8sEnabled: Boolean(body.kubevirt?.exec_enabled ?? body.kubevirt?.enabled),
    }
  } catch {
    return { k8sEnabled: false }
  }
}
