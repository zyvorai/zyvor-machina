// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { getFleetDigest, type FleetDigest } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import DigestScheduleControls from './DigestScheduleControls'

const HEADLINE: Record<FleetDigest['headline'], { text: string; cls: string }> = {
  ok: { text: 'All quiet', cls: 'text-emerald-500' },
  attention: { text: 'A few things need a look', cls: 'text-amber-500' },
  critical: { text: 'Needs attention now', cls: 'text-red-500' },
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div>
      <div className="text-lg font-semibold text-[var(--text-primary)]">{value}</div>
      <div className="text-xs text-[var(--text-muted)]">{label}</div>
    </div>
  )
}

/** Fleet briefing for a chosen window. Read-only. */
export default function FleetDigestPanel() {
  const [hours, setHours] = useState(24)
  const [digest, setDigest] = useState<FleetDigest | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    setError(null)
    getFleetDigest(hours)
      .then((d) => { if (live) setDigest(d) })
      .catch((e: unknown) => { if (live) { setDigest(null); setError(formatUserError(e)) } })
    return () => { live = false }
  }, [hours])

  const h = digest ? HEADLINE[digest.headline] : null
  return (
    <MacGlassPanel title="Fleet digest" subtitle="What happened, and what needs a person">
      <div className="flex items-center gap-2 mb-3 text-xs">
        <label htmlFor="digest-hours" className="text-[var(--text-muted)]">Window</label>
        <select id="digest-hours" className="input text-xs" value={hours} onChange={(e) => setHours(Number(e.target.value))}>
          <option value={24}>Last 24 hours</option>
          <option value={168}>Last 7 days</option>
        </select>
      </div>
      {error && <p className="text-xs text-red-500">{error}</p>}
      {digest && h && (
        <div data-testid="fleet-digest">
          <p className={`font-semibold mb-3 ${h.cls}`}>{h.text}</p>
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            <Stat label="Hosts not online" value={digest.hosts_not_online} />
            <Stat label="Open incidents" value={digest.open_incidents} />
            <Stat label="Pending approvals" value={digest.pending_approvals} />
            <Stat label="Zyra actions undone" value={digest.actions_undone} />
            <Stat label="Machines running" value={digest.vms_running} />
            <Stat label="Actions executed" value={digest.actions_executed} />
            <Stat label="Actions failed" value={digest.actions_failed} />
            <Stat label="Rejected by a person" value={digest.actions_rejected} />
          </div>
          {digest.budget_alerts.length > 0 && (
            <ul className="list-disc ml-4 mt-3 text-xs text-[var(--text-muted)]">
              {digest.budget_alerts.map((a) => <li key={a}>{a}</li>)}
            </ul>
          )}
        </div>
      )}
      <DigestScheduleControls />
    </MacGlassPanel>
  )
}
