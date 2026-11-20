// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { PlatformDesktopTier } from './platformDesktopTier'
import type { PlatformInfo } from '../api/system'
import type { DesktopHubTile } from './platformNavRegistry'
import {
  groupSpotlightByZone,
  hubTileById,
  spotlightEntriesForTier,
  spotlightPathSetForTier,
  spotlightZoneOrder,
  type SpotlightNavEntry,
} from './platformNavRegistry'

export type { SpotlightNavEntry }

export function spotlightNavForTier(tier: PlatformDesktopTier, info: PlatformInfo | null = null): SpotlightNavEntry[] {
  return spotlightEntriesForTier(tier, info)
}

export { groupSpotlightByZone, spotlightZoneOrder, spotlightPathSetForTier, hubTileById }
export type { DesktopHubTile }
