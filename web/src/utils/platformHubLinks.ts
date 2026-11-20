// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformDesktopTier } from './platformDesktopTier'
import { tierAtLeast } from './platformDesktopTier'
import { DESKTOP_HUB_TILES, type DesktopHubTile } from './platformHubZones'

/** Primary Operations entry — hub on Power+, Notification Center on Normal. */
export function operationsHubHref(tier: PlatformDesktopTier): string {
  return tierAtLeast(tier, 'power') ? '/platform/operations' : '/platform/notifications'
}

/** Task list entry — full tasks on Power+, alerts fallback on Normal. */
export function tasksHubHref(tier: PlatformDesktopTier): string {
  return tierAtLeast(tier, 'power') ? '/platform/tasks' : '/platform/notifications'
}

/** Activity monitor entry — hidden behind Power tier on Normal. */
export function activityHubHref(tier: PlatformDesktopTier): string {
  return tierAtLeast(tier, 'power') ? '/platform/activity' : '/platform/notifications'
}

/** Tier-aware href for a desktop hub tile (Operations falls back on Normal). */
export function hubHrefForTier(
  hubId: DesktopHubTile['id'] | 'resources' | 'integrations',
  tier: PlatformDesktopTier,
): string {
  if (hubId === 'operations') return operationsHubHref(tier)
  if (hubId === 'resources') return '/platform/infrastructure'
  if (hubId === 'integrations') {
    return '/platform/settings?section=integrations'
  }
  return DESKTOP_HUB_TILES.find((hub) => hub.id === hubId)?.href ?? '/platform'
}
