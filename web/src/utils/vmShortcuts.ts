// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { removePinnedVMs } from './pinnedVMs'
import { removeRecentVMs } from './recentVMs'

/** Drop deleted VM names from Spotlight recent/pinned shortcuts. */
export function purgeVmShortcuts(names: string[]) {
  const unique = [...new Set(names.filter(Boolean))]
  if (!unique.length) return
  removeRecentVMs(unique)
  removePinnedVMs(unique)
}
