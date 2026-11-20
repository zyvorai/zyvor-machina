// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { navGroups, routeLabels, navGroupItems } from './routes'

/** Human-readable label for an app route path. */
export function getPageLabel(path: string): string {
  const base = path.split('?')[0] || '/'
  if (routeLabels[base]) return routeLabels[base]

  for (const group of navGroups) {
    for (const item of navGroupItems(group)) {
      const itemPath = item.to.split('?')[0]
      if (itemPath === base) return item.label
    }
  }

  const segments = base.split('/').filter(Boolean)
  return segments[segments.length - 1] || 'Page'
}
