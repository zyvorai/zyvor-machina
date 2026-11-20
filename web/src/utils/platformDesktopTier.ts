// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// macOS-like desktop density — Normal · Power · Advanced.

export type PlatformDesktopTier = 'normal' | 'power' | 'advanced'

export const PLATFORM_DESKTOP_TIER_KEY = 'machina-platform-desktop-tier'
export const PLATFORM_DESKTOP_TIER_EVENT = 'machina-platform-desktop-tier-changed'

export const PLATFORM_DESKTOP_TIER_LABELS: Record<PlatformDesktopTier, string> = {
  normal: 'Normal',
  power: 'Power user',
  advanced: 'Advanced',
}

export const PLATFORM_DESKTOP_TIER_HINTS: Record<PlatformDesktopTier, string> = {
  normal: 'Clean desktop — Finder, hosts, backups, and settings.',
  power: 'Operations toolkit — storage, networks, Zyra, tasks, and shortcuts.',
  advanced: 'Full Machina fleet surface — every pane, firewall module, and admin tool.',
}

const TIER_RANK: Record<PlatformDesktopTier, number> = {
  normal: 0,
  power: 1,
  advanced: 2,
}

/** Paths always reachable (detail routes, popouts). */
const NORMAL_PATHS = [
  '/platform',
  '/platform/vms',
  '/platform/hosts',
  '/platform/storage',
  '/platform/backups',
  '/platform/settings',
  '/platform/support',
  '/platform/notifications',
]

/** Routes that require Advanced tier even when a parent prefix is allowed at Power. */
const ADVANCED_ONLY_PREFIXES = [
  '/platform/developer',
  '/platform/zeus/security/policies',
]

const POWER_PATHS = [
  ...NORMAL_PATHS,
  '/fleet-cloud',
  '/k8s',
  '/fleet',
  '/platform/applications',
  '/platform/infrastructure',
  '/platform/workloads',
  '/platform/administration',
  '/platform/resources',
  '/platform/operations',
  '/platform/datacenter',
  '/platform/storage',
  '/platform/networks',
  '/platform/gpu',
  '/platform/templates',
  '/platform/content',
  '/platform/tasks',
  '/platform/activity',
  '/platform/alert-rules',
  '/platform/scheduled-jobs',
  '/platform/migration',
  '/platform/blueprints',
  '/platform/projects',
  '/platform/zyra',
  '/platform/zeus/security',
  '/platform/zyra/security/hunt',
  '/platform/zyra/security/enforcement',
  '/platform/zyra/security/native-bpf',
  '/platform/zyra/security/network-policies',
  '/platform/zeus/security/firewall',
  '/platform/maintenance',
  '/platform/recommendations',
  '/platform/topology',
  '/platform/events',
  '/platform/enroll',
  '/platform/reports',
  '/platform/observability',
  '/platform/placement',
  '/platform/policy',
  '/platform/mission-control',
  '/platform/api-keys',
  '/platform/webhooks',
  '/platform/users',
  '/platform/enterprise',
]

export const DOCK_PATHS_BY_TIER: Record<PlatformDesktopTier, string[]> = {
  normal: ['/platform', '/platform/vms', '/platform/hosts', '/platform/storage', '/platform/networks', '/platform/zyra', '/platform/settings'],
  power: [
    '/platform',
    '/platform/vms',
    '/platform/hosts',
    '/platform/storage',
    '/platform/networks',
    '/platform/zyra',
    '/platform/infrastructure',
    '/platform/settings',
  ],
  advanced: [
    '/platform',
    '/platform/vms',
    '/platform/hosts',
    '/platform/storage',
    '/platform/networks',
    '/platform/zyra',
    '/platform/infrastructure',
    '/platform/operations',
    '/platform/settings',
  ],
}

function pathsForTier(tier: PlatformDesktopTier): string[] | null {
  if (tier === 'advanced') return null
  if (tier === 'power') return POWER_PATHS
  return NORMAL_PATHS
}

export function loadPlatformDesktopTier(): PlatformDesktopTier {
  try {
    const raw = localStorage.getItem(PLATFORM_DESKTOP_TIER_KEY)
    if (raw === 'normal' || raw === 'power' || raw === 'advanced') return raw
  } catch {
    /* ignore */
  }
  return 'normal'
}

export function savePlatformDesktopTier(tier: PlatformDesktopTier) {
  localStorage.setItem(PLATFORM_DESKTOP_TIER_KEY, tier)
  window.dispatchEvent(new CustomEvent(PLATFORM_DESKTOP_TIER_EVENT, { detail: tier }))
}

function isAdvancedOnlyPath(path: string): boolean {
  return ADVANCED_ONLY_PREFIXES.some((p) => path === p || path.startsWith(`${p}/`))
}

export function isPathAllowedForTier(path: string, tier: PlatformDesktopTier): boolean {
  if (tier === 'advanced') return true
  if (isAdvancedOnlyPath(path)) return false
  const allowed = pathsForTier(tier)!
  return allowed.some((p) => {
    if (path === p) return true
    // Dashboard root only — must not unlock every /platform/* child route.
    if (p === '/platform') return false
    return path.startsWith(`${p}/`)
  })
}

export function tierAtLeast(current: PlatformDesktopTier, min: PlatformDesktopTier): boolean {
  return TIER_RANK[current] >= TIER_RANK[min]
}
