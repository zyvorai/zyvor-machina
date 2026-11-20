// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { HelpTab } from '../components/HelpDialog'

export const OPEN_HELP_EVENT = 'machina-open-help'

export function dispatchOpenHelp(tab: HelpTab = 'shortcuts') {
  window.dispatchEvent(new CustomEvent(OPEN_HELP_EVENT, { detail: { tab } }))
}
