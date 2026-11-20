// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { ReactNode } from 'react'
import type { PlatformDesktopTier } from './platformDesktopTier'
import { sidebarForTier, sidebarLocationsOnly } from './platformNavFilter'
import type { PlatformNavItem, PlatformNavSection } from './platformNav'
import {
  administrationNavItemsForTier,
  infrastructureNavItemsForTier,
  operationsNavItemsForTier,
  securityNavItemsForTier,
  workloadsNavItemsForTier,
  zyraNavItemsForTier,
} from './platformNavRegistry'

type MacMenuNavItem = { to: string; label: string }

type MenubarProductSection = {
  label?: string
  items: MacMenuNavItem[]
}

/** Zeus-style top-level product menu (Workloads, Infra, …). */
export type MenubarProductGroup = {
  id: string
  /** Short menubar trigger label */
  compact: string
  sections: MenubarProductSection[]
}

/** True Advanced-only routes — still gated in the menubar on Normal/Power. */
const MENUBAR_ADVANCED_ONLY_PREFIXES = [
  '/platform/developer',
  '/platform/zeus/security/policies',
]

function pathKey(to: string): string {
  return to.split('?')[0] || to
}

function isMenubarPathAllowed(path: string, tier: PlatformDesktopTier): boolean {
  if (tier === 'advanced') return true
  return !MENUBAR_ADVANCED_ONLY_PREFIXES.some((p) => path === p || path.startsWith(`${p}/`))
}

function takeItems(
  candidates: MacMenuNavItem[],
  tier: PlatformDesktopTier,
  covered: Set<string>,
): MacMenuNavItem[] {
  const next: MacMenuNavItem[] = []
  for (const item of candidates) {
    if (!isMenubarPathAllowed(pathKey(item.to), tier)) continue
    if (covered.has(item.to)) continue
    covered.add(item.to)
    next.push({ to: item.to, label: item.label })
  }
  return next
}

function sectionFromItems(items: MacMenuNavItem[], label?: string): MenubarProductSection[] {
  if (items.length === 0) return []
  return [{ label, items }]
}

/**
 * Zeus-parity product menus — full hub + Host/Fleet/Platform catalogs for discovery.
 * Desktop tier no longer starves the menubar (Normal previously showed ~7 items).
 * Only Advanced-only prefixes stay gated.
 */
export function menubarProductGroupsForTier(
  tier: PlatformDesktopTier,
  integrationItems: PlatformNavItem[] = [],
): MenubarProductGroup[] {
  const covered = new Set<string>()
  const groups: MenubarProductGroup[] = []
  const catalogTier: PlatformDesktopTier = 'advanced'

  const workloads = takeItems(
    [
      ...workloadsNavItemsForTier(catalogTier),
      { to: '/vms', label: 'Virtual Machines' },
      { to: '/create', label: 'Create VM' },
      { to: '/containers', label: 'Containers' },
      { to: '/platform/templates', label: 'Templates' },
      { to: '/platform/fleet-snapshots', label: 'Snapshots' },
      { to: '/platform/blueprints', label: 'Blueprints' },
    ],
    tier,
    covered,
  )
  if (workloads.length > 0) {
    groups.push({ id: 'workloads', compact: 'Workloads', sections: sectionFromItems(workloads) })
  }

  const infra = takeItems(
    [
      ...infrastructureNavItemsForTier(catalogTier),
      { to: '/networks', label: 'Host Networks' },
      { to: '/storage', label: 'Host Storage' },
    ],
    tier,
    covered,
  )
  if (infra.length > 0) {
    groups.push({ id: 'infra', compact: 'Infra', sections: sectionFromItems(infra) })
  }

  const ops = takeItems(operationsNavItemsForTier(catalogTier), tier, covered)
  if (ops.length > 0) {
    groups.push({ id: 'ops', compact: 'Ops', sections: sectionFromItems(ops) })
  }

  const secure = takeItems(securityNavItemsForTier(catalogTier), tier, covered)
  if (secure.length > 0) {
    groups.push({ id: 'secure', compact: 'Secure', sections: sectionFromItems(secure) })
  }

  const admin = takeItems(
    [
      ...administrationNavItemsForTier(catalogTier),
      { to: '/platform/settings', label: 'Settings' },
      { to: '/platform/enterprise', label: 'Enterprise' },
      { to: '/platform/api-keys', label: 'API Keys' },
      { to: '/platform/webhooks', label: 'Webhooks' },
    ],
    tier,
    covered,
  )
  if (admin.length > 0) {
    groups.push({ id: 'admin', compact: 'Admin', sections: sectionFromItems(admin) })
  }

  const moreSections: MenubarProductSection[] = []

  const favorites = takeItems(
    sidebarForTier('normal', [])
      .flatMap((section) => section.items)
      .map((item) => ({ to: item.to, label: item.label })),
    tier,
    covered,
  )
  if (favorites.length > 0) {
    moreSections.push({ label: 'Favorites', items: favorites })
  }

  // Full Host / Fleet / Platform rail leftovers (not Normal favorites-only).
  const locationSections = sidebarLocationsOnly(sidebarForTier('advanced', integrationItems))
  for (const section of locationSections) {
    const items = takeItems(
      section.items.map((item) => ({ to: item.to, label: item.label })),
      tier,
      covered,
    )
    if (items.length > 0) {
      moreSections.push({ label: section.label, items })
    }
  }

  const zyra = takeItems(zyraNavItemsForTier(catalogTier), tier, covered)
  if (zyra.length > 0) {
    moreSections.push({ label: 'Zyra', items: zyra })
  }

  const integrations = takeItems(
    integrationItems.map((item) => ({ to: item.to, label: item.label })),
    tier,
    covered,
  )
  if (integrations.length > 0) {
    moreSections.push({ label: 'Connected', items: integrations })
  }

  if (moreSections.length > 0) {
    groups.push({ id: 'more', compact: 'More', sections: moreSections })
  }

  return groups
}

/** Flat sections derived from product menus (Spotlight / legacy callers). */
export function macMenuSectionsForTier(
  tier: PlatformDesktopTier,
  integrationItems: PlatformNavItem[] = [],
): PlatformNavSection[] {
  return menubarProductGroupsForTier(tier, integrationItems).map((group) => ({
    label: group.compact,
    items: group.sections.flatMap((section) =>
      section.items.map((item) => ({
        to: item.to,
        label: item.label,
        icon: null as ReactNode,
        subsection: section.label,
      })),
    ),
  }))
}

export function flattenMacMenuForTier(
  tier: PlatformDesktopTier,
  integrationItems: PlatformNavItem[] = [],
): MacMenuNavItem[] {
  return menubarProductGroupsForTier(tier, integrationItems).flatMap((group) =>
    group.sections.flatMap((section) => section.items),
  )
}
