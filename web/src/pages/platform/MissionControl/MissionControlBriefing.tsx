// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useNavigate } from 'react-router'
import { useEffect, useState } from 'react'
import { getFleetSummary } from '../../../api/ai'
import { ZYRA_ASSISTANT_NAME } from '../../../config/aiBrand'
import { dispatchOpenSpotlight } from '../../../utils/platformJarvisShell'
import type { MissionControlFleetState } from './useMissionControlFleet'

type Props = {
  state: MissionControlFleetState
  missingImagesCount?: number
  onAnalyze?: () => void
}

export default function MissionControlBriefing({ state, missingImagesCount = 0, onAnalyze }: Props) {
  const navigate = useNavigate()
  const [summaryText, setSummaryText] = useState<string | null>(null)
  const { unprotected, needsAttention } = state

  useEffect(() => {
    void getFleetSummary()
      .then((r) =>
        setSummaryText(
          `${r.aggregate_vm_count} VMs across ${r.reachable_peers}/${r.peer_count} peers. Est. $${r.aggregate_monthly_usd.toFixed(0)}/mo.`,
        ),
      )
      .catch(() => setSummaryText(null))
  }, [state.vms.length])

  const links = [
    missingImagesCount > 0 ? { label: 'Missing golden images', action: () => navigate('/platform/vm-builder') } : null,
    unprotected > 0 ? { label: `${unprotected} need backup`, action: () => navigate('/platform/vms?folder=unprotected') } : null,
    needsAttention > 0 ? { label: 'Analyze fleet', action: () => onAnalyze?.() ?? dispatchOpenSpotlight('analyze fleet health') } : null,
    { label: 'Recovery', action: () => navigate('/platform/backups') },
  ].filter(Boolean) as { label: string; action: () => void }[]

  const controlPlaneDown = Boolean(state.error) && !state.loading

  return (
    <section className="apple-section" data-testid="mission-control-briefing">
      <p className="apple-eyebrow">{ZYRA_ASSISTANT_NAME}</p>
      <h2 className="apple-display apple-display--sm">
        {state.loading ? 'Scanning fleet…' : controlPlaneDown ? 'Control plane unreachable' : 'Fleet at a glance'}
      </h2>
      <p className="apple-lede">
        {state.loading
          ? 'Gathering host and guest signals.'
          : controlPlaneDown
            ? 'Fleet briefing pauses until machina-controller responds.'
            : summaryText ?? state.finder?.summary ?? 'Fleet summary unavailable.'}
      </p>
      {!controlPlaneDown && links.length > 0 && (
        <nav className="apple-cta-row" aria-label="Briefing actions">
          {links.map((chip) => (
            <button key={chip.label} type="button" className="apple-text-link" onClick={chip.action}>
              {chip.label} <span aria-hidden>›</span>
            </button>
          ))}
        </nav>
      )}
    </section>
  )
}
