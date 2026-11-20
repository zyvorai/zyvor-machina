// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { PLATFORM_PAGE_LABELS } from './platformNav'

type PlatformDesktopTab = { path: string; label: string }

const KEY = 'machina-platform-desktop-tabs'
const PLATFORM_DESKTOP_TABS_EVENT = 'machina-platform-desktop-tabs-changed'
const MAX_PLATFORM_DESKTOP_TABS = 12

const VM_UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

/** One macOS-style window per app hub — child routes replace the same tab instead of duplicating labels. */
export function platformDesktopTabGroup(pathname: string): string {
  const path = pathname.split('?')[0]
  if (path === '/platform' || path === '/platform/') return '/platform'
  if (path.startsWith('/platform/vms')) return '/platform/vms'
  if (path.startsWith('/platform/hosts/finder')) return '/platform/vms'
  if (path.startsWith('/platform/hosts')) return '/platform/hosts'
  if (path.startsWith('/platform/settings')) return '/platform/settings'
  if (path.startsWith('/platform/applications')) return '/platform/applications'
  if (path.startsWith('/platform/zeus/security')) return '/platform/zeus/security'
  if (path.startsWith('/platform/zyra')) return '/platform/zyra'
  const parts = path.split('/').filter(Boolean)
  if (parts.length >= 3) return `/${parts.slice(0, 3).join('/')}`
  return path
}

function normalizePlatformDesktopTabs(tabs: PlatformDesktopTab[]): PlatformDesktopTab[] {
  const byGroup = new Map<string, PlatformDesktopTab>()
  for (const tab of tabs) {
    const group = platformDesktopTabGroup(tab.path)
    byGroup.set(group, { path: group, label: tab.label })
  }
  return Array.from(byGroup.values()).slice(-MAX_PLATFORM_DESKTOP_TABS)
}

export function loadPlatformDesktopTabs(): PlatformDesktopTab[] {
  try {
    const raw = localStorage.getItem(KEY)
    if (raw) {
      const parsed = JSON.parse(raw) as PlatformDesktopTab[]
      if (Array.isArray(parsed) && parsed.length) {
        const normalized = normalizePlatformDesktopTabs(parsed)
        if (normalized.length !== parsed.length) {
          save(normalized)
        }
        return normalized
      }
    }
  } catch {
    /* ignore */
  }
  return [{ path: '/platform', label: 'Mission Control' }]
}

function save(tabs: PlatformDesktopTab[]) {
  const trimmed = tabs.slice(-MAX_PLATFORM_DESKTOP_TABS)
  localStorage.setItem(KEY, JSON.stringify(trimmed))
  window.dispatchEvent(new CustomEvent(PLATFORM_DESKTOP_TABS_EVENT))
}

export function upsertPlatformDesktopTab(tab: PlatformDesktopTab): PlatformDesktopTab[] {
  const group = platformDesktopTabGroup(tab.path)
  // One window per app hub — store the hub path so dock / Window menus never reopen stale child URLs.
  const canonical: PlatformDesktopTab = { path: group, label: tab.label }
  const rest = loadPlatformDesktopTabs().filter((t) => platformDesktopTabGroup(t.path) !== group)
  const next = normalizePlatformDesktopTabs([...rest, canonical])
  save(next)
  return next
}

export function platformPageLabel(pathname: string): string {
  const path = pathname.split('?')[0]
  const parts = path.split('/').filter(Boolean)

  if (parts[0] === 'platform' && parts[1] === 'vms') {
    if (parts.length === 2) return 'Machine Finder'
    if (parts[2] === 'create') return 'New VM'
    if (parts.length >= 4 && parts[3] === 'console') return 'VM Console'
    if (parts.length >= 3 && VM_UUID_RE.test(parts[2] ?? '')) return 'Virtual Machine'
  }

  if (parts[0] === 'platform' && parts[1] === 'hosts') {
    if (parts[2] === 'finder') return 'Machine Finder'
    if (parts.length >= 3 && VM_UUID_RE.test(parts[2] ?? '')) return 'Host Details'
  }

  const base = parts.length >= 3 ? `/${parts.slice(0, 3).join('/')}` : '/platform'
  return PLATFORM_PAGE_LABELS[path] ?? PLATFORM_PAGE_LABELS[base] ?? 'Platform'
}
