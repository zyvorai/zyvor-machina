// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { MacGlassPanel, MacListRow } from './mac/PlatformMacUi'
import type { SecurityEvent } from '../../api/zeusSecurity'

export default function SecurityTimelinePanel({
  events,
  title = 'Security timeline',
  subtitle = 'Unified flight recorder — events + correlations',
}: {
  events: SecurityEvent[]
  title?: string
  subtitle?: string
}) {
  return (
    <MacGlassPanel title={title} subtitle={subtitle}>
      {events.length === 0 ? (
        <p className="text-sm text-[var(--text-muted)]">No security events in this window.</p>
      ) : (
        <div className="rounded-xl border border-white/[0.06] overflow-clip">
          {events.slice(0, 40).map((e, i) => (
            <MacListRow
              key={e.id ?? i}
              title={e.summary || e.message || e.kind || e.type || 'event'}
              subtitle={[e.host_id, e.source, e.namespace, e.severity, e.timestamp].filter(Boolean).join(' · ')}
            />
          ))}
        </div>
      )}
    </MacGlassPanel>
  )
}
