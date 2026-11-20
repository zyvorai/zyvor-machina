// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useMemo } from 'react'
import { useLocation } from 'react-router'
import { usePlatformInfoSlow } from '../contexts/PlatformInfoContext'
import { usePlatformDesktopTier } from './usePlatformDesktopTier'
import { integrationNavItems } from '../utils/platformIntegrationsNav'
import { menubarProductGroupsForTier } from '../utils/platformMacMenus'
import { navItemActive } from '../utils/routes'

const EYEBROW: Record<string, string> = {
  workloads: 'Workloads',
  infra: 'Infrastructure',
  ops: 'Operations',
  secure: 'Security',
  admin: 'Administration',
}

/**
 * Eyebrow for a page header, taken from the top-bar group that contains the current route
 * (undefined for routes outside the groups, and for the catch-all "More" group).
 */
export function useNavGroupEyebrow(): string | undefined {
  const { pathname, search } = useLocation()
  const { info } = usePlatformInfoSlow()
  const [tier] = usePlatformDesktopTier()
  return useMemo(() => {
    const groups = menubarProductGroupsForTier(tier, integrationNavItems(info))
    for (const g of groups) {
      const label = EYEBROW[g.id]
      if (!label) continue
      if (g.sections.some((s) => s.items.some((i) => navItemActive({ to: i.to, label: i.label, icon: null }, pathname, search)))) return label
    }
    return undefined
  }, [pathname, search, tier, info])
}
