// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useRef, useState, type ReactElement } from 'react'
import { CheckCircle2, Circle, Copy, Loader2, XCircle } from 'lucide-react'
import FleetCloudMap, { type MapHost } from './FleetCloudMap'
import MacTerminal from './MacTerminal'
import { getJoinProgress, listPlatformHosts } from '../../api/platform'
import { copyText } from '../../utils/copyText'
import { joinStages, maskToken, stageElapsed, terminalLines, type JoinProgress, type StageStatus } from '../../utils/joinProgress'

type Props = {
  token: string
  /** The command the operator was shown; echoed (token masked) as the first terminal line. */
  command?: string
  /** Terminal height in px (default 300). */
  terminalHeight?: number
  /** Called with every poll result, so a parent can show the outcome (joined / failed). */
  onProgress?: (p: JoinProgress) => void
}

const ICON: Record<StageStatus, ReactElement> = {
  done: <CheckCircle2 className="w-4 h-4 text-[#30d158]" aria-hidden />,
  active: <Loader2 className="w-4 h-4 text-[#0a84ff] animate-spin" aria-hidden />,
  pending: <Circle className="w-4 h-4 text-[var(--text-muted)]" aria-hidden />,
  error: <XCircle className="w-4 h-4 text-[#ff453a]" aria-hidden />,
}

/** Live view of a host joining: stage tracker, the fleet map with the new node, and the controller's log. */
export default function JoinLivePanel({ token, command, terminalHeight = 300, onProgress }: Props) {
  const [progress, setProgress] = useState<JoinProgress | null>(null)
  const [hosts, setHosts] = useState<MapHost[]>([])
  const [problem, setProblem] = useState<string | null>(null)
  const finished = useRef(false)
  const progressCb = useRef(onProgress)
  progressCb.current = onProgress

  useEffect(() => {
    finished.current = false
    let stop = false
    const tick = async () => {
      try {
        const [p, hs] = await Promise.all([getJoinProgress(token), listPlatformHosts().catch(() => null)])
        if (stop) return
        setProgress(p)
        progressCb.current?.(p)
        if (hs) setHosts(hs)
        setProblem(null)
        const stages = joinStages(p)
        if (stages.every((s) => s.status === 'done') || stages.some((s) => s.status === 'error') || p.token_status === 'expired') finished.current = true
      } catch (e) {
        if (!stop) setProblem(e instanceof Error ? e.message : 'could not read progress')
      }
    }
    void tick()
    const t = window.setInterval(() => { if (!finished.current) void tick() }, 2000)
    return () => { stop = true; window.clearInterval(t) }
  }, [token])

  const stages = useMemo(() => joinStages(progress), [progress])
  const elapsed = useMemo(() => stageElapsed(progress?.events ?? []), [progress])
  const lines = useMemo(() => terminalLines(progress?.events ?? []), [progress])
  const joinedHost = progress?.host ?? null
  const failed = stages.some((s) => s.status === 'error')
  const complete = stages.every((s) => s.status === 'done')
  const contactLabel = progress?.events.find((e) => e.step === 'contact')?.message.split(' contacted')[0]

  // The joined host is already in the list once registered; before that it is drawn as the pulsing node.
  const knownIds = new Set(hosts.map((h) => h.id))
  const joining = complete || (joinedHost && knownIds.has(joinedHost.id)) ? null : { label: contactLabel ?? 'waiting for the host…', failed }

  return (
    <div className="space-y-4" data-testid="join-live-panel">
      <ol className="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm" aria-label="Join progress">
        {stages.map((s) => (
          <li key={s.id} data-stage={s.id} data-status={s.status} className="inline-flex items-center gap-1.5 text-[var(--text-secondary)]">
            {ICON[s.status]}
            <span className={s.status === 'pending' ? 'text-[var(--text-muted)]' : 'text-[var(--text-primary)]'}>{s.label}</span>
            {s.status !== 'pending' && elapsed[s.id] != null && <span className="text-xs tabular-nums text-[var(--text-muted)]">+{elapsed[s.id]} s</span>}
          </li>
        ))}
      </ol>
      <FleetCloudMap hosts={hosts} joining={joining} highlightId={complete ? joinedHost?.id : null} height={300} />
      <MacTerminal
        height={terminalHeight}
        title="machina — host join"
        prompt={command ? maskToken(command, token) : undefined}
        lines={lines}
        waiting="waiting for the host to run the command…"
        live={!complete && !failed && progress?.token_status !== 'expired'}
        footer={problem ? `controller unreachable: ${problem}` : progress?.token_status === 'expired' ? 'enrollment token expired — create a new one' : complete ? 'done — the host is part of the fleet' : failed ? 'join failed — see the red lines above' : 'watching…'}
      />
      <div className="flex justify-end">
        <button
          type="button"
          className="btn-secondary text-xs inline-flex items-center gap-1"
          disabled={lines.length === 0}
          onClick={() => void copyText(lines.map((l) => `${l.time} ${l.symbol} ${l.text}`).join('\n'))}
        >
          <Copy className="w-3 h-3" aria-hidden /> Copy log
        </button>
      </div>
    </div>
  )
}
