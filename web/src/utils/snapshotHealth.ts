// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export function snapshotStateSeverity(state: string): 'ok' | 'warn' | 'error' | 'info' {
  const s = state?.toLowerCase() ?? ''
  if (s === 'error' || s === 'crashed') return 'error'
  if (s === 'blocking' || s === 'paused') return 'warn'
  return 'info'
}
