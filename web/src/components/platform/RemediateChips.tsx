// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Sparkles } from 'lucide-react'
import { getComplianceRemediate, getSreRemediate } from '../../api/ai'
import { MacGlassPanel } from './mac/PlatformMacUi'

type Remediation = { label: string; review: string; action?: string; framework?: string }

const VISIBLE_COMPACT = 3

export default function RemediateChips({ compact = false }: { compact?: boolean }) {
  const [sre, setSre] = useState<Remediation[]>([])
  const [compliance, setCompliance] = useState<Remediation[]>([])
  const [summary, setSummary] = useState<string | null>(null)
  const [expanded, setExpanded] = useState(false)

  const load = useCallback(async () => {
    try {
      const [s, c] = await Promise.all([getSreRemediate(), getComplianceRemediate()])
      setSre(s.remediations ?? [])
      setCompliance(c.remediations ?? [])
      setSummary([s.summary, c.summary].filter(Boolean).join(' · ') || null)
    } catch {
      setSre([])
      setCompliance([])
      setSummary(null)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const items = [
    ...sre.map((r) => ({ ...r, hub: '/platform/zyra' as const })),
    ...compliance.map((r) => ({ ...r, hub: '/platform/zeus/security/compliance' as const })),
  ]
  if (items.length === 0 && !summary) return null

  const visibleCount = compact ? (expanded ? items.length : VISIBLE_COMPACT) : 12
  const hiddenCount = items.length - VISIBLE_COMPACT

  const chipList = (
    <div className="tahoe-remediate-chips flex flex-wrap items-center gap-2">
      {items.slice(0, visibleCount).map((r, i) => (
        <Link
          key={`${r.label}-${i}`}
          to={r.hub}
          className="tahoe-remediate-chip group"
          title={r.review}
        >
          <Sparkles className="w-3 h-3 shrink-0 opacity-0 group-hover:opacity-70 transition-opacity" />
          <span className="truncate max-w-[14rem]">{r.label}</span>
          {r.framework ? <span className="text-[var(--text-muted)] text-[10px]">({r.framework})</span> : null}
        </Link>
      ))}
      {compact && hiddenCount > 0 && !expanded ? (
        <button
          type="button"
          className="tahoe-remediate-more"
          onClick={() => setExpanded(true)}
        >
          +{hiddenCount} more
        </button>
      ) : null}
      {compact && expanded && items.length > VISIBLE_COMPACT ? (
        <button type="button" className="tahoe-remediate-more" onClick={() => setExpanded(false)}>
          Show less
        </button>
      ) : null}
      {compact ? (
        <Link to="/platform/zyra" className="tahoe-remediate-more ml-auto shrink-0">
          Remediation hub →
        </Link>
      ) : null}
    </div>
  )

  if (compact) {
    return (
      <div className="tahoe-remediate-strip">
        {summary ? <p className="tahoe-remediate-summary">{summary}</p> : null}
        {chipList}
      </div>
    )
  }

  return (
    <MacGlassPanel title="AI remediations" subtitle="SRE + compliance suggestions from controller">
      {summary && <p className="text-xs text-[var(--text-muted)] mb-2">{summary}</p>}
      {chipList}
    </MacGlassPanel>
  )
}
