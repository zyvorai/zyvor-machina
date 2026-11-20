// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test'
import { mockAuthenticatedApi } from './helpers/authMock'

async function mockUnauthenticatedApi(page: import('@playwright/test').Page) {
  await page.route('**/api/v1/**', async (route) => {
    const url = route.request().url()
    if (url.includes('/auth/providers')) {
      return route.fulfill({
        json: {
          pam: { enabled: true },
          ldap: { enabled: false },
          oidc: { enabled: false, button_label: 'Sign in with SSO' },
        },
      })
    }
    if (url.includes('/auth/session')) {
      return route.fulfill({ status: 401, json: { error: 'unauthenticated' } })
    }
    return route.fulfill({ status: 401, json: { error: 'unauthenticated', error_code: 'unauthorized' } })
  })
}

test('login page shows PAM form when OIDC is off', async ({ page }) => {
  await mockUnauthenticatedApi(page)
  await page.goto('/login')
  await expect(page.getByText('Machina').first()).toBeVisible()
  await expect(page.getByLabel('Username')).toBeVisible()
})

test('authenticated /login redirects to dashboard', async ({ page }) => {
  await mockAuthenticatedApi(page)
  await page.goto('/login')
  await expect(page).toHaveURL('/', { timeout: 15_000 })
  await expect(page.locator('#main-content')).toBeVisible({ timeout: 15_000 })
})

test('VM list shows empty state when authenticated', async ({ page }) => {
  await mockAuthenticatedApi(page)
  await page.goto('/vms')
  await expect(page.getByRole('heading', { name: /virtual machines/i })).toBeVisible({ timeout: 15_000 })
})

test('Fleet page loads when authenticated', async ({ page }) => {
  await mockAuthenticatedApi(page)
  await page.goto('/fleet')
  await expect(page.getByRole('heading', { name: 'Fleet', exact: true })).toBeVisible({ timeout: 15_000 })
})

// The login page's language switcher (and all useTranslation()/i18n usage in Login.tsx generally)
// was removed when the login flow was rebuilt as PremiumLoginShell's single centered composition —
// LanguageSwitcher.tsx is only used by the orphaned classic Navbar.tsx now. No replacement to test.

test('Fleet Cloud instances shows sanitized error when API returns HTML', async ({ page }) => {
  await mockAuthenticatedApi(page)
  // Override the (already-mocked) native VM list endpoint for just this test, so the
  // page has to handle a raw HTML error body instead of the happy-path JSON.
  await page.route(/\/platform\/controller\/api\/v1\/vms(\?|$)/, async (route) => {
    return route.fulfill({
      status: 503,
      contentType: 'text/html',
      body: '<!DOCTYPE html><html><body>Bad Gateway</body></html>',
    })
  })
  await page.goto('/fleet-cloud/instances')
  // Both the error heading and the detail body text start with "Failed to load instances".
  await expect(page.getByText(/Failed to load instances/i).first()).toBeVisible({ timeout: 15_000 })
  await expect(page.getByText(/HTML error page/i).first()).toBeVisible()
})
