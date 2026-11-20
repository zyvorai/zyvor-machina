// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { expect, type Page } from '@playwright/test'
import { ensureLoggedIn, liveCredentials, setDesktopTier } from './liveAuth'

export const liveBaseUrl = () => process.env.PLAYWRIGHT_LIVE_URL?.replace(/\/$/, '') ?? ''

/** The libvirt domain name — used for daemon API paths (/api/v1/vms/{name}). */
export const liveVmId = () => process.env.PLAYWRIGHT_LIBVIRT_VM_ID?.trim() ?? ''

export const liveVmName = () => process.env.PLAYWRIGHT_LIBVIRT_VM_NAME?.trim() || 'ubuntu-desktop'

// Primary tabs have role="tab" (explicit ARIA role on <button> elements inside tablist).
const PRIMARY_TABS = new Set(['Overview', 'Access', 'Hardware', 'Console', 'Performance', 'Doctor', 'Disks', 'Devices'])

export function skipUnlessLiveVm(test: { skip: (cond: boolean, reason?: string) => void }) {
  test.skip(!liveBaseUrl() || !liveCredentials(), 'Set PLAYWRIGHT_LIVE_URL, PLAYWRIGHT_LIVE_USER, PLAYWRIGHT_LIVE_PASS')
  test.skip(!liveVmId(), 'Set PLAYWRIGHT_LIBVIRT_VM_ID')
}

/**
 * Resolve the controller UUID for the VM.
 * The platform UI routes (/platform/vms/{id}) require a UUID, not a libvirt domain name.
 * We call the controller VM list, find the VM by name, and cache the UUID.
 */
let _platformVmId = ''

export async function livePlatformVmId(page: Page, nameHint = liveVmId()): Promise<string> {
  if (_platformVmId) return _platformVmId

  // If already looks like a UUID, use directly
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(nameHint)) {
    _platformVmId = nameHint
    return nameHint
  }

  // Discover from controller VM list
  try {
    const live = liveBaseUrl()
    const res = await page.request.get(
      `${live}/api/v1/platform/controller/api/v1/vms`,
      { ignoreHTTPSErrors: true },
    )
    if (res.ok()) {
      const vms = (await res.json()) as Array<{ id: string; name?: string }>
      const vmName = liveVmName()
      const found = vms.find((v) => v.name === nameHint || v.name === vmName)
      if (found?.id) {
        _platformVmId = found.id
        return found.id
      }
    }
  } catch { /* fall through */ }

  // Fallback: use the name as-is and hope the backend accepts it
  _platformVmId = nameHint
  return nameHint
}

/**
 * Open the platform VM detail page for the configured VM.
 * Automatically resolves the controller UUID (the UI requires UUID, not domain name).
 * Login is established here so subsequent page.request calls carry the session cookie.
 */
export async function openLiveVmDetail(page: Page, vmId = liveVmId(), vmName = liveVmName()) {
  const live = liveBaseUrl()
  await setDesktopTier(page, 'power')
  // { navigate: false } avoids an intermediate /platform hop — we go straight to the VM page below
  await ensureLoggedIn(page, live, { navigate: false })
  // Resolve UUID after login (API call needs auth cookie)
  const platformId = await livePlatformVmId(page, vmId)
  await page.goto(`${live}/platform/vms/${platformId}`, { waitUntil: 'domcontentloaded', timeout: 180_000 })
  await expect(page.getByText(new RegExp(vmName, 'i')).first()).toBeVisible({ timeout: 120_000 })
}

/**
 * Primary tabs have role="tab"; More-menu tabs are role="menuitem".
 * The "More" trigger button label changes to the active tab name when a More tab is selected,
 * so we locate it by aria-haspopup="menu" inside the tablist instead of by text.
 */
export async function openVmDetailTab(page: Page, label: string) {
  if (PRIMARY_TABS.has(label)) {
    await page.getByRole('tab', { name: label, exact: true }).click()
    return
  }
  await page.locator('[role="tablist"] button[aria-haspopup="menu"]').click()
  await page.getByRole('menuitem', { name: label, exact: true }).click()
}

export async function platformApiGet(page: Page, path: string) {
  return page.request.get(`${liveBaseUrl()}${path}`, { ignoreHTTPSErrors: true })
}

export async function platformApiPost(page: Page, path: string, body?: unknown) {
  return page.request.post(`${liveBaseUrl()}${path}`, {
    ignoreHTTPSErrors: true,
    data: body,
  })
}

export async function platformApiDelete(page: Page, path: string) {
  return page.request.delete(`${liveBaseUrl()}${path}`, { ignoreHTTPSErrors: true })
}

/**
 * Poll the daemon VM API until the VM reaches the expected state (e.g. 'running', 'shutoff', 'paused').
 * Uses the session cookie established by the current page context.
 */
export async function waitForVmState(page: Page, state: string, timeoutMs = 60_000) {
  const live = liveBaseUrl()
  const vmId = liveVmId()
  await expect
    .poll(
      async () => {
        const r = await page.request.get(`${live}/api/v1/vms/${vmId}`, { ignoreHTTPSErrors: true })
        if (!r.ok()) return null
        const body = (await r.json()) as { state?: string }
        return body.state ?? null
      },
      { timeout: timeoutMs, intervals: [2000] },
    )
    .toBe(state)
}

/**
 * Poll the controller's platform API until the VM's observed_state matches.
 * 'running' maps to 'running'; shutoff maps to 'shutoff'/'stopped'/'shut off'.
 * Uses a loose match so both 'shutoff' and 'stopped'/'shut off' all satisfy 'stopped'.
 */
export async function waitForControllerVmState(page: Page, state: string, timeoutMs = 90_000) {
  const live = liveBaseUrl()
  const platformId = await livePlatformVmId(page)
  const stopped = new Set(['stopped', 'shut off', 'shutoff', 'defined'])
  await expect
    .poll(
      async () => {
        const r = await page.request.get(
          `${live}/api/v1/platform/controller/api/v1/vms/${platformId}`,
          { ignoreHTTPSErrors: true },
        )
        if (!r.ok()) return null
        const body = (await r.json()) as { observed_state?: string }
        const obs = body.observed_state ?? null
        if (!obs) return null
        if (state === 'running') return obs === 'running' ? 'running' : obs
        if (state === 'shutoff' || state === 'stopped') return stopped.has(obs) ? state : obs
        return obs
      },
      { timeout: timeoutMs, intervals: [2000] },
    )
    .toBe(state)
}

/**
 * Issue a power action to a VM via the platform controller API.
 * action: 'start' | 'shutdown' | 'pause' | 'resume' | 'reboot' | 'stop'
 */
export async function controllerVmPower(page: Page, action: string) {
  const live = liveBaseUrl()
  const platformId = await livePlatformVmId(page)
  return page.request.post(
    `${live}/api/v1/platform/controller/api/v1/vms/${platformId}/${action}`,
    { ignoreHTTPSErrors: true },
  )
}

/**
 * Poll the controller snapshot list until the named snapshot is no longer present.
 * Does NOT fire a delete request — use this after the UI already triggered deletion.
 */
export async function waitForControllerVmSnapshotGone(page: Page, snapName: string, timeoutMs = 90_000) {
  const live = liveBaseUrl()
  const platformId = await livePlatformVmId(page)
  await expect
    .poll(
      async () => {
        const r = await page.request.get(
          `${live}/api/v1/platform/controller/api/v1/vms/${platformId}/snapshots`,
          { ignoreHTTPSErrors: true },
        )
        // A failed request tells us nothing about whether the snapshot is gone —
        // keep polling (and let the overall timeout surface a real failure) instead
        // of treating "can't confirm" as "confirmed deleted".
        if (!r.ok()) return false
        const snaps = (await r.json()) as Array<{ name?: string; status?: string }>
        return !snaps.some((s) => s.name === snapName)
      },
      { timeout: timeoutMs, intervals: [3000] },
    )
    .toBe(true)
}

/**
 * Adopt the VM into platform management (sets managed=true).
 * Safe to call when already managed — the 400 response is silently ignored.
 * Call before any test that needs to click hardware mutation buttons (Attach disk,
 * Detach disk, Resize, Attach NIC, Detach NIC, Edit hardware).
 */
export async function ensureVmManaged(page: Page) {
  const live = liveBaseUrl()
  const platformId = await livePlatformVmId(page)
  await page.request.post(
    `${live}/api/v1/platform/controller/api/v1/vms/${platformId}/adopt`,
    { ignoreHTTPSErrors: true },
  )
  // 200 = adopted now, 400 = already managed — both are acceptable
}

/**
 * Poll the controller's libvirt-details API until check() returns true.
 * Use this after async disk/NIC operations to wait for libvirt state to reflect the change.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function pollControllerLibvirtDetails(page: Page, check: (details: any) => boolean, timeoutMs = 90_000) {
  const live = liveBaseUrl()
  const platformId = await livePlatformVmId(page)
  await expect
    .poll(
      async () => {
        const r = await page.request.get(
          `${live}/api/v1/platform/controller/api/v1/vms/${platformId}/libvirt-details`,
          { ignoreHTTPSErrors: true },
        )
        if (!r.ok()) return false
        try {
          // eslint-disable-next-line @typescript-eslint/no-unsafe-return
          return check(await r.json())
        } catch {
          return false
        }
      },
      { timeout: timeoutMs, intervals: [3000] },
    )
    .toBe(true)
}

/**
 * Delete a VM snapshot by name via the controller API and poll until it is no longer
 * in the snapshot list. Safe to call even if the snapshot does not exist (404 is ignored).
 */
export async function deleteControllerVmSnapshot(page: Page, snapName: string, timeoutMs = 90_000) {
  const live = liveBaseUrl()
  const platformId = await livePlatformVmId(page)
  // Fire the delete (may 404 if doesn't exist)
  await page.request.delete(
    `${live}/api/v1/platform/controller/api/v1/vms/${platformId}/snapshots/${encodeURIComponent(snapName)}`,
    { ignoreHTTPSErrors: true },
  )
  // Poll until the snapshot no longer appears in the list
  await expect
    .poll(
      async () => {
        const r = await page.request.get(
          `${live}/api/v1/platform/controller/api/v1/vms/${platformId}/snapshots`,
          { ignoreHTTPSErrors: true },
        )
        // A failed request tells us nothing about whether the snapshot is gone —
        // keep polling (and let the overall timeout surface a real failure) instead
        // of treating "can't confirm" as "confirmed deleted".
        if (!r.ok()) return false
        const snaps = (await r.json()) as Array<{ name?: string; status?: string }>
        return !snaps.some((s) => s.name === snapName)
      },
      { timeout: timeoutMs, intervals: [3000] },
    )
    .toBe(true)
}
