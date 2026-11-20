// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { Activity, Cpu, Sparkles } from 'lucide-react'
import { statusBadgeClasses } from '../../utils/semanticColors'

type Props = {
  vmName: string
  vmState?: string | null
  guestIp?: string | null
  osHint?: string
  nodeName?: string | null
  uptime?: string | null
  healthScore?: number | null
  theatre?: boolean
  onEnterTheatre?: () => void
  onPopout?: () => void
  onCommandCenter?: () => void
  onAi?: () => void
}

function stateTone(state?: string | null): 'ok' | 'warn' | 'error' | 'neutral' {
  const s = (state ?? '').toLowerCase()
  if (s.includes('run') || s === 'active') return 'ok'
  if (s.includes('migrat') || s.includes('pause')) return 'warn'
  if (s.includes('fail') || s.includes('error') || s.includes('crash')) return 'error'
  if (s.includes('stop') || s.includes('shut')) return 'neutral'
  return 'neutral'
}

export default function MachineCommandStrip({
  vmName,
  vmState,
  guestIp,
  osHint,
  nodeName,
  uptime,
  healthScore,
  theatre,
  onEnterTheatre,
  onPopout,
  onCommandCenter,
  onAi,
}: Props) {
  const tone = stateTone(vmState)
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-1 py-2 border-b border-white/[0.06] text-sm shrink-0">
      <div className="flex items-center gap-2 min-w-0">
        <span className={`w-2 h-2 rounded-full shrink-0 ${tone === 'ok' ? 'bg-emerald-400' : tone === 'warn' ? 'bg-amber-400' : tone === 'error' ? 'bg-red-400' : 'bg-slate-500'}`} />
        <span className="font-semibold text-[var(--text-primary)] truncate">{vmName}</span>
        {vmState ? <span className={`text-xs px-2 py-0.5 rounded-full ${statusBadgeClasses(tone)}`}>{vmState}</span> : null}
      </div>
      {guestIp ? <span className="font-mono text-emerald-600/90 text-xs">{guestIp}</span> : null}
      {osHint && osHint !== 'unknown' ? <span className="text-xs text-[var(--text-muted)]">{osHint}</span> : null}
      {nodeName ? <span className="text-xs text-[var(--text-muted)]">Node {nodeName}</span> : null}
      {uptime ? <span className="text-xs text-[var(--text-muted)]">{uptime}</span> : null}
      {healthScore != null ? (
        <span className="inline-flex items-center gap-1 text-xs text-[var(--text-muted)]">
          <Activity className="w-3.5 h-3.5" /> Health {healthScore}
        </span>
      ) : null}
      <div className="flex-1" />
      <div className="flex items-center gap-2 max-w-full overflow-x-auto [scrollbar-width:none] [&>*]:shrink-0">
        {!theatre && onEnterTheatre ? (
          <button type="button" className="btn-secondary text-xs py-1 px-2" onClick={onEnterTheatre}>
            Open Cinema
          </button>
        ) : null}
        {onPopout ? (
          <button type="button" className="btn-secondary text-xs py-1 px-2" onClick={onPopout}>
            Pop out
          </button>
        ) : null}
        {onCommandCenter ? (
          <button type="button" className="btn-secondary text-xs py-1 px-2 inline-flex items-center gap-1" onClick={onCommandCenter}>
            <Cpu className="w-3.5 h-3.5" /> Ops Shelf
          </button>
        ) : null}
        {onAi ? (
          <button type="button" className="btn-secondary text-xs py-1 px-2 inline-flex items-center gap-1" onClick={onAi}>
            <Sparkles className="w-3.5 h-3.5" /> AI
          </button>
        ) : null}
      </div>
    </div>
  )
}
