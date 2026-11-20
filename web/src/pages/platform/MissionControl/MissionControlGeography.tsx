// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { ChevronDown, ChevronUp } from 'lucide-react'
import InfrastructureEarthView from '../../../components/platform/InfrastructureEarthView'
import { getFleetMission, type FleetMissionOverview } from '../../../api/platform'
import { formatUserError } from '../../../utils/apiError'

type Props = {
  expanded?: boolean
  onToggle?: () => void
}

export default function MissionControlGeography({ expanded: expandedProp, onToggle }: Props) {
  const [internalExpanded, setInternalExpanded] = useState(false)
  const expanded = expandedProp ?? internalExpanded
  const [mission, setMission] = useState<FleetMissionOverview | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setError(null)
    try {
      setMission(await getFleetMission())
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => {
    if (expanded) void load()
  }, [expanded, load])

  const toggle = () => {
    if (onToggle) onToggle()
    else setInternalExpanded((v) => !v)
  }

  return (
    <section id="geography" className="mc-geography rounded-2xl border border-white/[0.08] bg-[var(--apple-surface)] overflow-hidden" data-testid="mission-control-geography">
      <button
        type="button"
        className="w-full flex items-center justify-between px-4 py-3 text-left hover:bg-[var(--surface-hover)]"
        onClick={toggle}
      >
        <div>
          <h2 className="text-sm font-semibold text-[var(--text-primary)]">Fleet geography</h2>
          <p className="text-xs text-[var(--text-muted)]">Site, rack, host topology</p>
        </div>
        {expanded ? <ChevronUp className="w-4 h-4 text-[var(--text-muted)]" /> : <ChevronDown className="w-4 h-4 text-[var(--text-muted)]" />}
      </button>
      {expanded && (
        <div className="px-4 pb-4 border-t border-white/[0.06]">
          {error && <p className="text-sm text-red-600 py-2">{error}</p>}
          {mission && <InfrastructureEarthView mission={mission} />}
          {!mission && !error && <p className="text-sm text-[var(--text-muted)] py-4">Loading geography…</p>}
        </div>
      )}
    </section>
  )
}
