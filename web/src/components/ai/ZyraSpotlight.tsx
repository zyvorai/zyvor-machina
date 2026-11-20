// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import CommandPalette from '../CommandPalette'
import type { HelpTab } from '../HelpDialog'

/** Unified ⌘Space command bar — Zyra Spotlight (⌘K alias via CommandPalette). */
export default function ZyraSpotlight({ onOpenHelp }: { onOpenHelp?: (tab?: HelpTab) => void }) {
  return <CommandPalette onOpenHelp={onOpenHelp} spotlight />
}
