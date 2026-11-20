// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Loader2 } from 'lucide-react'
import { MacGlassPanel } from './mac/PlatformMacUi'
import { useFleetSettings, type FleetSettingsKind } from '../../hooks/useFleetSettings'

const TITLES: Record<FleetSettingsKind, string> = {
  network: 'Fleet network',
  storage: 'Fleet storage',
  console: 'Fleet console',
  updates: 'Fleet updates',
  keychain: 'Fleet keychain',
  users: 'Fleet users',
  shortcuts: 'Fleet shortcuts',
  spaces: 'Fleet spaces',
  general: 'Fleet general',
}

export default function FleetSettingsPane({ kind, enabled = true }: { kind: FleetSettingsKind; enabled?: boolean }) {
  const { data, loading, error } = useFleetSettings(kind, enabled)

  if (!enabled) return null

  const summary = data && typeof data === 'object' && 'summary' in data
    ? String((data as { summary?: string }).summary ?? '')
    : null

  return (
    <MacGlassPanel title={TITLES[kind]} subtitle="Fleet-wide summary">
      {loading && (
        <p className="text-sm text-[var(--text-muted)] flex items-center gap-2">
          <Loader2 className="w-4 h-4 animate-spin" /> Loading fleet summary…
        </p>
      )}
      {error && !loading && (
        <p className="text-sm text-[var(--text-muted)]">Fleet summary is temporarily unavailable. Page data above may still be current.</p>
      )}
      {!loading && !error && summary && <p className="text-sm text-[var(--text-secondary)] leading-relaxed">{summary}</p>}
      {!loading && !error && !summary && data && (
        <p className="text-sm text-[var(--text-muted)]">Fleet aggregate loaded.</p>
      )}
    </MacGlassPanel>
  )
}
