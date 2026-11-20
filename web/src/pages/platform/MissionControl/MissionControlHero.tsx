// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { AlertTriangle, Search } from 'lucide-react'
import { Link } from 'react-router'
import { formatFleetDisplayTitle } from '../../../utils/fleetDisplayName'
import { dispatchOpenSpotlight } from '../../../utils/platformJarvisShell'
import type { MissionControlFleetState } from './useMissionControlFleet'

type Props = {
  state: MissionControlFleetState
  warnings: number
  onCreateVm: () => void
}

export default function MissionControlHero({ state, warnings, onCreateVm }: Props) {
  const { cluster, hosts, running, onlineHosts, storagePct, needsAttention, attentionMode, setAttentionMode } = state
  const fleetTitle = formatFleetDisplayTitle(cluster, hosts)
  const healthy = !state.loading && hosts.length > 0 && warnings === 0 && onlineHosts === hosts.length

  const controlPlaneDown = Boolean(state.error)
  const status: 'connecting' | 'offline' | 'degraded' | 'operational' = state.loading
    ? 'connecting'
    : controlPlaneDown
      ? 'offline'
      : healthy
        ? 'operational'
        : 'degraded'
  const word = {
    connecting: 'Connecting…',
    offline: 'Control plane offline',
    degraded: 'Degraded',
    operational: 'Operational',
  }[status]

  const hour = new Date().getHours()
  const hello = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening'

  return (
    <>
      <header className="apple-section apple-hero-band mc-hero mc-hero-ribbon nl-aurora-host" data-testid="mission-control-hero" data-status={status}>
        <span className="nl-aurora" aria-hidden />
        <p className="apple-eyebrow">Machina · {fleetTitle}</p>
        <p className="text-[17px] text-[var(--text-secondary)] tracking-tight mb-2 text-center">{hello}</p>
        <h1 className="apple-display">{word}</h1>
        <p className="apple-lede">
          {controlPlaneDown
            ? 'Machina controller is unreachable — inventory and fleet metrics are unavailable until it recovers.'
            : 'Your private cloud control plane. Guests live in Machine Finder — this home stays calm.'}
        </p>
        <div className="apple-cta-row">
          <button type="button" onClick={onCreateVm} className="btn-primary">
            New VM
          </button>
          <Link to="/platform/vms" className="apple-text-link">
            Machine Finder <span aria-hidden>›</span>
          </Link>
          <button type="button" className="apple-text-link" onClick={() => dispatchOpenSpotlight()}>
            <Search className="w-4 h-4" />
            Search
            <span className="font-mono text-[12px] text-[var(--text-muted)] ml-1">⌘K</span>
          </button>
          {needsAttention > 0 && (
            <button
              type="button"
              className={`apple-text-link ${attentionMode ? 'text-[var(--machina-status-warn)]' : ''}`}
              onClick={() => setAttentionMode(!attentionMode)}
              data-testid="attention-mode-toggle"
            >
              <AlertTriangle className="w-4 h-4" />
              {needsAttention} need attention
            </button>
          )}
        </div>
        {!state.loading && !controlPlaneDown ? (
          <ul className="nl-chip-row" aria-label="Fleet status">
            <li className="nl-chip" data-tone={onlineHosts === hosts.length && hosts.length > 0 ? 'ok' : 'warn'}><i aria-hidden />{onlineHosts}/{hosts.length} hosts online</li>
            <li className="nl-chip" data-tone="info"><i aria-hidden />{running} VMs running</li>
            <li className="nl-chip" data-tone={warnings > 0 ? 'warn' : 'ok'}><i aria-hidden />{warnings > 0 ? `${warnings} alert${warnings === 1 ? '' : 's'}` : 'No alerts'}</li>
          </ul>
        ) : null}
      </header>

    </>
  )
}
