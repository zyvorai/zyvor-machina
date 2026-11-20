// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformDesktopTier } from './platformDesktopTier'
import { isPathAllowedForTier } from './platformDesktopTier'
import { NORMAL_FAVORITE_PATHS, PLATFORM_SIDEBAR, type PlatformNavItem, type PlatformNavSection } from './platformNav'

function withIntegrations(sections: PlatformNavSection[], extra: PlatformNavItem[]): PlatformNavSection[] {
  if (extra.length === 0) return sections
  return [...sections, { label: 'Connected platforms', items: extra, collapsible: true, defaultCollapsed: false }]
}

/** Favorites duplicate the dock — Finder sidebar keeps Host / Fleet / Platform only. */
export function sidebarLocationsOnly(sections: PlatformNavSection[]): PlatformNavSection[] {
  return sections.filter((s) => s.label !== 'Favorites')
}

export function sidebarForTier(tier: PlatformDesktopTier, integrationItems: PlatformNavItem[] = []): PlatformNavSection[] {
  if (tier === 'normal') {
    const favorites = PLATFORM_SIDEBAR.flatMap((s) => s.items).filter((item) =>
      (NORMAL_FAVORITE_PATHS as readonly string[]).includes(item.to),
    )
    return [{ label: 'Favorites', items: favorites }]
  }

  if (tier === 'advanced') return withIntegrations(PLATFORM_SIDEBAR, integrationItems)

  const filtered = PLATFORM_SIDEBAR.map((section) => ({
    ...section,
    items: section.items.filter((item) => isPathAllowedForTier(item.to, tier)),
  })).filter((section) => section.items.length > 0)

  return withIntegrations(filtered, integrationItems.filter((item) => {
    if (item.to.startsWith('/platform')) return isPathAllowedForTier(item.to, tier)
    return true
  }))
}
