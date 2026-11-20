// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Single source of truth for platform hub context nav and Spotlight entries.

import type { LucideIcon } from 'lucide-react'
import { Cpu, LayoutGrid, Monitor, Package, Server, Settings, ShieldAlert, Wrench } from 'lucide-react'
import type { PlatformDesktopTier } from './platformDesktopTier'
import { isPathAllowedForTier, tierAtLeast } from './platformDesktopTier'
import { platformPageLabel } from './platformDesktopTabs'
import { hubHrefForTier } from './platformHubLinks'
import { integrationNavItems } from './platformIntegrationsNav'
import { sidebarForTier } from './platformNavFilter'
import type { PlatformInfo } from '../api/system'

export type ContextNavItem = {
  to: string
  label: string
}

export type PlatformContextNav = {
  appLabel: string
  appIcon: LucideIcon
  hubPath?: string
  items: ContextNavItem[]
}

export type SpotlightNavEntry = {
  id: string
  label: string
  path: string
  zone: string
  description?: string
  kind: 'hub' | 'destination'
}

type DesktopHubId = 'infrastructure' | 'workloads' | 'operations' | 'administration' | 'security'

export type DesktopHubTile = {
  id: DesktopHubId
  label: string
  href: string
  description: string
  zone: 'Favorites' | 'Fleet' | 'Platform'
}

type ContextDefinition = {
  id: string
  match: (pathname: string) => boolean
  appLabel: string
  appIcon: LucideIcon
  hubPath?: string
  items: ContextNavItem[]
  spotlightZone?: string
}

type SettingsNavEntry = ContextNavItem & { minTier?: PlatformDesktopTier }

const SETTINGS_ENTRIES: SettingsNavEntry[] = [
  { to: '/platform/settings?section=general', label: 'General' },
  { to: '/platform/settings?section=security', label: 'Security' },
  { to: '/platform/settings?section=network', label: 'Network' },
  { to: '/platform/settings?section=users', label: 'Users' },
  { to: '/platform/settings?section=stage-manager', label: 'Stage Manager', minTier: 'power' },
  { to: '/platform/settings?section=keychain', label: 'Keychain', minTier: 'power' },
  { to: '/platform/settings?section=policy', label: 'Policy', minTier: 'power' },
  { to: '/platform/settings?section=api-keys', label: 'API Keys', minTier: 'power' },
  { to: '/platform/settings?section=webhooks', label: 'Webhooks', minTier: 'power' },
  { to: '/platform/settings?section=reports', label: 'Reports', minTier: 'power' },
  { to: '/platform/settings?section=console', label: 'Console', minTier: 'power' },
  { to: '/platform/settings?section=resources', label: 'Infrastructure', minTier: 'power' },
  { to: '/platform/settings?section=updates', label: 'Updates' },
  { to: '/platform/settings?section=integrations', label: 'Integrations' },
  // Support / About live in the top menubar Help menu — not the Settings side rail.
]

const SECURITY_ITEMS: ContextNavItem[] = [
  { to: '/platform/soc', label: 'SOC' },
  { to: '/platform/zeus/security', label: 'Security Center' },
  { to: '/platform/zeus/security/firewall', label: 'Firewall' },
  { to: '/platform/zeus/security/ports', label: 'Open Ports' },
  { to: '/platform/zeus/security/services', label: 'Allowed Apps' },
  { to: '/platform/zeus/security/activity', label: 'Activity' },
  { to: '/platform/zeus/security/compliance', label: 'Compliance' },
  { to: '/platform/zeus/security/k8s', label: 'Kubernetes' },
  { to: '/platform/zeus/security/cloud', label: 'Cloud SGs' },
  { to: '/platform/zeus/security/connectivity', label: 'Connectivity' },
  { to: '/platform/zeus/security/policies', label: 'Policy Studio' },
  { to: '/platform/zyra/security/hunt', label: 'Threat Hunting' },
  { to: '/platform/zyra/security/enforcement', label: 'Enforcement' },
  { to: '/platform/zyra/security/native-bpf', label: 'Native eBPF' },
  { to: '/platform/zyra/security/network-policies', label: 'VM Network Policies' },
]

const INFRASTRUCTURE_ITEMS: ContextNavItem[] = [
  { to: '/platform/infrastructure', label: 'Overview' },
  { to: '/platform/datacenter', label: 'Clusters' },
  { to: '/platform/hosts', label: 'Hosts' },
  { to: '/platform/storage', label: 'Storage' },
  { to: '/platform/networks', label: 'Networks' },
  { to: '/platform/reports', label: 'Capacity' },
  { to: '/platform/gpu', label: 'GPU Command Center' },
  { to: '/platform/content', label: 'Images & ISOs' },
  { to: '/platform/templates', label: 'Templates' },
  { to: '/platform/cloud-init', label: 'Cloud-Init Studio' },
]

const WORKLOADS_ITEMS: ContextNavItem[] = [
  { to: '/platform/workloads', label: 'Overview' },
  { to: '/platform/applications', label: 'Applications' },
  { to: '/platform/vms', label: 'Machine Finder' },
  { to: '/k8s/workloads', label: 'Kubernetes Workloads' },
]

const ADMINISTRATION_ITEMS: ContextNavItem[] = [
  { to: '/platform/administration', label: 'Overview' },
  { to: '/platform/users', label: 'Users' },
  { to: '/platform/projects', label: 'Projects' },
  { to: '/platform/policy', label: 'Policies' },
  { to: '/platform/settings?section=integrations', label: 'Integrations' },
]

const OPERATIONS_ITEMS: ContextNavItem[] = [
  { to: '/platform/operations', label: 'Overview' },
  { to: '/platform/activity', label: 'Monitoring' },
  { to: '/platform/events', label: 'Events' },
  { to: '/platform/backups', label: 'Backups' },
  { to: '/platform/maintenance', label: 'Upgrades' },
  { to: '/platform/tasks', label: 'Tasks' },
  { to: '/platform/notifications', label: 'Alerts' },
  { to: '/platform/alert-rules', label: 'Alert Rules' },
  { to: '/platform/scheduled-jobs', label: 'Scheduled Jobs' },
  { to: '/platform/migration', label: 'Migration' },
  { to: '/platform/fleet-snapshots', label: 'Fleet Snapshots' },
  { to: '/platform/network-canvas', label: 'Network Canvas' },
  { to: '/platform/placement', label: 'Disaster Recovery' },
  { to: '/platform/reports', label: 'Reports' },
  { to: '/platform/topology', label: 'Topology' },
  { to: '/platform/observability', label: 'Observability' },
  { to: '/platform/recommendations', label: 'Recommendations' },
  { to: '/platform/blueprints', label: 'Shortcuts' },
]

const ZEUS_ITEMS: ContextNavItem[] = [
  { to: '/platform/zyra', label: 'Overview' },
  { to: '/platform/zyra?tab=fleet', label: 'Fleet AI' },
  { to: '/platform/zyra?tab=services', label: 'Services' },
  { to: '/platform/zyra?tab=security', label: 'Attack paths' },
  { to: '/platform/zyra?tab=knowledge', label: 'Knowledge' },
  { to: '/platform/zyra?tab=baremetal', label: 'Bare Metal' },
  { to: '/platform/zeus/security', label: 'Security Center' },
]

/** Hub launchpads — deep routes live inside these pages, not as duplicate chrome. */
export const DESKTOP_HUB_TILES: DesktopHubTile[] = [
  {
    id: 'infrastructure',
    label: 'Infrastructure',
    href: '/platform/infrastructure',
    description: 'Clusters, hosts, storage, networks, and capacity',
    zone: 'Platform',
  },
  {
    id: 'workloads',
    label: 'Workloads',
    href: '/platform/workloads',
    description: 'Applications, VMs, and Kubernetes objects',
    zone: 'Platform',
  },
  {
    id: 'operations',
    label: 'Operations',
    href: '/platform/operations',
    description: 'Monitoring, events, backups, and upgrades',
    zone: 'Platform',
  },
  {
    id: 'administration',
    label: 'Administration',
    href: '/platform/administration',
    description: 'Users, projects, policies, and integrations',
    zone: 'Platform',
  },
  {
    id: 'security',
    label: 'Security Center',
    href: '/platform/zeus/security',
    description: 'Threat posture, firewall, and Zeus security workspaces',
    zone: 'Platform',
  },
]

const HUB_DEFINITIONS: ContextDefinition[] = [
  {
    id: 'security',
    match: (p) => p.startsWith('/platform/zeus/security'),
    appLabel: 'Security',
    appIcon: ShieldAlert,
    hubPath: '/platform/zeus/security',
    items: SECURITY_ITEMS,
    spotlightZone: 'Security',
  },
  {
    id: 'zyra',
    match: (p) => p === '/platform/zyra' || p.startsWith('/platform/zyra/'),
    appLabel: 'Machina Zyra OS',
    appIcon: Cpu,
    hubPath: '/platform/zyra',
    items: ZEUS_ITEMS,
    spotlightZone: 'Zyra',
  },
  {
    id: 'infrastructure',
    match: (p) =>
      p === '/platform/infrastructure'
      || p === '/platform/resources'
      || p.startsWith('/platform/datacenter')
      || p.startsWith('/platform/storage')
      || p.startsWith('/platform/networks')
      || p.startsWith('/platform/content')
      || p.startsWith('/platform/templates')
      || p.startsWith('/platform/cloud-init')
      || p.startsWith('/platform/gpu'),
    appLabel: 'Infrastructure',
    appIcon: Server,
    hubPath: '/platform/infrastructure',
    items: INFRASTRUCTURE_ITEMS,
    spotlightZone: 'Infrastructure',
  },
  {
    id: 'workloads',
    match: (p) =>
      p === '/platform/workloads'
      || p.startsWith('/platform/applications'),
    appLabel: 'Workloads',
    appIcon: Package,
    hubPath: '/platform/workloads',
    items: WORKLOADS_ITEMS,
    spotlightZone: 'Workloads',
  },
  {
    id: 'operations',
    match: (p) =>
      p === '/platform/operations'
      || p.startsWith('/platform/tasks')
      || p.startsWith('/platform/notifications')
      || p.startsWith('/platform/alert-rules')
      || p.startsWith('/platform/scheduled-jobs')
      || p.startsWith('/platform/activity')
      || p.startsWith('/platform/events')
      || p.startsWith('/platform/migration')
      || p.startsWith('/platform/backups')
      || p.startsWith('/platform/fleet-snapshots')
      || p.startsWith('/platform/network-canvas')
      || p.startsWith('/platform/placement')
      || p.startsWith('/platform/maintenance')
      || p.startsWith('/platform/reports')
      || p.startsWith('/platform/recommendations')
      || p.startsWith('/platform/blueprints')
      || p.startsWith('/platform/topology')
      || p.startsWith('/platform/observability'),
    appLabel: 'Operations',
    appIcon: Wrench,
    hubPath: '/platform/operations',
    items: OPERATIONS_ITEMS,
    spotlightZone: 'Operations',
  },
  {
    id: 'administration',
    match: (p) =>
      p === '/platform/administration'
      || p.startsWith('/platform/users')
      || p.startsWith('/platform/projects')
      || p.startsWith('/platform/policy')
      || p.startsWith('/platform/integrations')
      || p.startsWith('/platform/api-keys')
      || p.startsWith('/platform/webhooks')
      || p.startsWith('/platform/enterprise'),
    appLabel: 'Administration',
    appIcon: Settings,
    hubPath: '/platform/administration',
    items: ADMINISTRATION_ITEMS,
    spotlightZone: 'Administration',
  },
  {
    id: 'settings',
    match: (p) => p.startsWith('/platform/settings'),
    appLabel: 'Settings',
    appIcon: Settings,
    hubPath: '/platform/settings',
    items: [],
    spotlightZone: 'Settings',
  },
  {
    id: 'finder',
    match: (p) => p.startsWith('/platform/vms'),
    appLabel: 'Finder',
    appIcon: Monitor,
    hubPath: '/platform/vms',
    items: [{ to: '/platform/vms', label: 'Machine Finder' }],
  },
  {
    id: 'hosts',
    match: (p) => p.startsWith('/platform/hosts'),
    appLabel: 'Hosts',
    appIcon: Server,
    hubPath: '/platform/hosts',
    items: [
      { to: '/platform/hosts', label: 'Machines' },
      { to: '/platform/vms?lens=topology', label: 'Topology lens' },
      { to: '/platform/enroll', label: 'Add Host' },
    ],
  },
]

/** Full-page settings workspaces linked from inline Settings panes. */
export const SETTINGS_WORKSPACE_PATHS: Record<string, string> = {
  '/platform/users': 'users',
  '/platform/projects': 'stage-manager',
  '/platform/enterprise': 'keychain',
  '/platform/policy': 'policy',
  '/platform/api-keys': 'api-keys',
  '/platform/webhooks': 'webhooks',
  '/platform/events': 'console',
}

export function isSettingsWorkspacePath(pathname: string): boolean {
  return pathname in SETTINGS_WORKSPACE_PATHS
}

export function isSettingsContextPath(pathname: string): boolean {
  return pathname.startsWith('/platform/settings') || isSettingsWorkspacePath(pathname)
}

function settingsSectionFromPath(pathname: string, search: string): string {
  if (pathname.startsWith('/platform/settings')) {
    return new URLSearchParams(search).get('section') ?? 'general'
  }
  return SETTINGS_WORKSPACE_PATHS[pathname] ?? 'general'
}

function filterItems(items: ContextNavItem[], tier: PlatformDesktopTier): ContextNavItem[] {
  return items.filter((item) => {
    const path = item.to.split('?')[0]
    return isPathAllowedForTier(path, tier)
  })
}

export function settingsItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return SETTINGS_ENTRIES
    .filter((item) => tierAtLeast(tier, item.minTier ?? 'normal'))
    .map(({ to, label }) => ({ to, label }))
}

export function operationsNavItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return filterItems(OPERATIONS_ITEMS, tier)
}

export function infrastructureNavItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return filterItems(INFRASTRUCTURE_ITEMS, tier)
}

export function workloadsNavItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return filterItems(WORKLOADS_ITEMS, tier)
}

export function administrationNavItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return filterItems(ADMINISTRATION_ITEMS, tier)
}

export function securityNavItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return filterItems(SECURITY_ITEMS, tier)
}

export function zyraNavItemsForTier(tier: PlatformDesktopTier): ContextNavItem[] {
  return filterItems(ZEUS_ITEMS, tier)
}

export function hubTilesForTier(tier: PlatformDesktopTier): DesktopHubTile[] {
  return DESKTOP_HUB_TILES.filter((tile) => isPathAllowedForTier(tile.href, tier))
}

export function hubTileById(id: DesktopHubId): DesktopHubTile | undefined {
  return DESKTOP_HUB_TILES.find((hub) => hub.id === id)
}

/** Routes that use in-page DetailTabs — context bar would duplicate section nav. */
const DETAIL_TABS_EXACT = new Set([
  '/platform/storage',
  '/platform/networks',
  '/platform/zyra',
  '/platform/maintenance',
  '/platform/templates',
  '/platform/blueprints',
  '/platform/migration',
  '/platform/reports',
  '/platform/enterprise',
  '/platform/settings',
  '/platform/backups',
])

const POWER_CONTEXT_HUB_ROOTS = new Set([
  '/platform/infrastructure',
  '/platform/workloads',
  '/platform/operations',
  '/platform/administration',
  '/platform/zeus/security',
  '/platform/settings',
])

export function suppressContextBar(pathname: string): boolean {
  if (DETAIL_TABS_EXACT.has(pathname)) return true
  if (/^\/platform\/vms\/[^/]+$/.test(pathname)) return true
  if (/^\/platform\/hosts\/[^/]+$/.test(pathname)) return true
  if (/^\/platform\/zyra\/machines\/[^/]+$/.test(pathname)) return true
  return false
}

/** Tier-aware visibility for the shell context bar (Wave 3). */
export function shouldShowContextBar(pathname: string, tier: PlatformDesktopTier): boolean {
  if (suppressContextBar(pathname)) return false
  if (tier === 'normal') return false
  if (tier === 'power') return POWER_CONTEXT_HUB_ROOTS.has(pathname)
  return true
}

export function contextNavForPath(pathname: string, tier: PlatformDesktopTier): PlatformContextNav | null {
  let def = HUB_DEFINITIONS.find((d) => d.match(pathname))
  if (isSettingsWorkspacePath(pathname)) {
    def = HUB_DEFINITIONS.find((d) => d.id === 'settings')
  }
  if (!def) {
    const label = platformPageLabel(pathname)
    if (pathname === '/platform') {
      return {
        appLabel: 'Mission Control',
        appIcon: LayoutGrid,
        items: [],
      }
    }
    return {
      appLabel: label,
      appIcon: LayoutGrid,
      items: [],
    }
  }

  const items = isSettingsContextPath(pathname)
    ? settingsItemsForTier(tier)
    : filterItems(def.items, tier)
  if (items.length <= 1) {
    return {
      appLabel: def.appLabel,
      appIcon: def.appIcon,
      hubPath: def.hubPath,
      items: [],
    }
  }

  return {
    appLabel: def.appLabel,
    appIcon: def.appIcon,
    hubPath: def.hubPath,
    items,
  }
}

export function isContextNavActive(pathname: string, search: string, item: ContextNavItem): boolean {
  const [itemPath, itemQuery] = item.to.split('?')

  if (itemQuery) {
    const expected = new URLSearchParams(itemQuery)
    const current = new URLSearchParams(search)

    if (itemPath === '/platform/settings') {
      if (!isSettingsContextPath(pathname)) return false
      const expectedSection = expected.get('section') ?? 'general'
      return settingsSectionFromPath(pathname, search) === expectedSection
    }

    if (pathname !== itemPath && !pathname.startsWith(`${itemPath}/`)) return false
    for (const [key, value] of expected.entries()) {
      if (current.get(key) !== value) return false
    }
    return true
  }

  const exactHubPaths = [
    '/platform/zeus/security',
    '/platform/operations',
    '/platform/infrastructure',
    '/platform/workloads',
    '/platform/administration',
    '/platform/settings',
    '/platform/zyra',
  ]
  if (exactHubPaths.includes(itemPath)) {
    return pathname === itemPath
  }

  return pathname === itemPath || pathname.startsWith(`${itemPath}/`)
}

export const MAX_CONTEXT_PILLS = 6

export function splitContextNavItems(
  items: ContextNavItem[],
  pathname: string,
  search: string,
  maxVisible = MAX_CONTEXT_PILLS,
): { visible: ContextNavItem[]; overflow: ContextNavItem[] } {
  if (items.length <= maxVisible) return { visible: items, overflow: [] }

  const activeIndex = items.findIndex((item) => isContextNavActive(pathname, search, item))
  const primary = items.slice(0, maxVisible - 1)
  const active = activeIndex >= 0 ? items[activeIndex] : null

  if (active && !primary.some((item) => item.to === active.to)) {
    const visible = [...primary, active]
    const visiblePaths = new Set(visible.map((item) => item.to))
    return {
      visible,
      overflow: items.filter((item) => !visiblePaths.has(item.to)),
    }
  }

  return {
    visible: items.slice(0, maxVisible),
    overflow: items.slice(maxVisible),
  }
}

export function spotlightZoneOrder(): string[] {
  return [
    'Platform hubs',
    'Settings',
    'Infrastructure',
    'Workloads',
    'Operations',
    'Administration',
    'Security',
    'Zyra',
    'Favorites',
    'Fleet',
    'Platform',
    'Connected platforms',
  ]
}

const SPOTLIGHT_HUB_ZONES: Array<{
  zone: string
  itemsForTier: (tier: PlatformDesktopTier) => ContextNavItem[]
  description: string
}> = [
  { zone: 'Settings', itemsForTier: settingsItemsForTier, description: 'Settings workspace' },
  { zone: 'Infrastructure', itemsForTier: infrastructureNavItemsForTier, description: 'Infrastructure workspace' },
  { zone: 'Workloads', itemsForTier: workloadsNavItemsForTier, description: 'Workloads workspace' },
  { zone: 'Operations', itemsForTier: operationsNavItemsForTier, description: 'Operations workspace' },
  { zone: 'Administration', itemsForTier: administrationNavItemsForTier, description: 'Administration workspace' },
  { zone: 'Security', itemsForTier: securityNavItemsForTier, description: 'Security workspace' },
  { zone: 'Zyra', itemsForTier: zyraNavItemsForTier, description: 'Zyra workspace' },
]

/** Spotlight / Command palette entries grouped by sidebar zone. */
export function spotlightEntriesForTier(tier: PlatformDesktopTier, info: PlatformInfo | null = null): SpotlightNavEntry[] {
  const entries: SpotlightNavEntry[] = []
  const seen = new Set<string>()

  const push = (entry: SpotlightNavEntry) => {
    if (seen.has(entry.path)) return
    seen.add(entry.path)
    entries.push(entry)
  }

  for (const hub of DESKTOP_HUB_TILES) {
    if (!isPathAllowedForTier(hub.href, tier)) continue
    push({
      id: `hub-${hub.id}`,
      label: hub.label,
      path: hubHrefForTier(hub.id, tier),
      zone: 'Platform hubs',
      description: hub.description,
      kind: 'hub',
    })
  }

  for (const { zone, itemsForTier, description } of SPOTLIGHT_HUB_ZONES) {
    for (const item of itemsForTier(tier)) {
      const itemDescription =
        item.to.includes('section=users') || item.to === '/platform/users'
          ? 'Add user · RBAC roles · platform accounts'
          : description
      push({
        id: `${zone.toLowerCase()}-${item.label}`,
        label: item.label,
        path: item.to,
        zone,
        description: itemDescription,
        kind: 'destination',
      })
    }
  }

  for (const section of sidebarForTier(tier, integrationNavItems(info))) {
    for (const item of section.items) {
      if (!isPathAllowedForTier(item.to, tier)) continue
      push({
        id: `nav-${item.to}`,
        label: item.label,
        path: item.to,
        zone: section.label,
        kind: 'destination',
      })
    }
  }

  return entries
}

export function groupSpotlightByZone(entries: SpotlightNavEntry[]): Array<{ zone: string; items: SpotlightNavEntry[] }> {
  const order = spotlightZoneOrder()
  const buckets = new Map<string, SpotlightNavEntry[]>()
  for (const entry of entries) {
    const list = buckets.get(entry.zone) ?? []
    list.push(entry)
    buckets.set(entry.zone, list)
  }
  return order
    .filter((zone) => buckets.has(zone))
    .map((zone) => ({ zone, items: buckets.get(zone)! }))
}

/** All Spotlight paths including query strings — for CommandPalette dedupe. */
export function spotlightPathSetForTier(tier: PlatformDesktopTier, info: PlatformInfo | null = null): Set<string> {
  return new Set(spotlightEntriesForTier(tier, info).map((entry) => entry.path))
}
