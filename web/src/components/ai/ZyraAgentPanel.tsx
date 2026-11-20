// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Loader2, Sparkles, Wrench } from 'lucide-react'
import { AgentStreamUnsupported, runZyraAgent, streamZyraAgent, type ZyraAgentRun, type ZyraAgentStep } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'

const SUGGESTIONS = [
  'Which machines have no backup?',
  'Is anything unhealthy right now?',
  'Make sure my running machines are protected',
]

const TOOL_LABELS: Record<string, string> = {
  list_vms: 'Looked at machines',
  list_hosts: 'Looked at hosts',
  recent_events: 'Read recent events',
  find_idle_vms: 'Looked for idle machines',
  plan_environment: 'Planned an environment',
  propose_action: 'Queued a proposal',
}

/**
 * Ask Zyra in plain words. It looks at the real fleet and can queue proposals — it never changes anything
 * itself; every proposal waits in the approvals queue for a human.
 */
export default function ZyraAgentPanel({ onProposed }: { onProposed?: () => void }) {
  const [prompt, setPrompt] = useState('')
  const [busy, setBusy] = useState(false)
  const [run, setRun] = useState<ZyraAgentRun | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [live, setLive] = useState<ZyraAgentStep[]>([])

  const go = async (text: string) => {
    const q = text.trim()
    if (!q || busy) return
    setBusy(true)
    setError(null)
    setRun(null)
    setLive([])
    try {
      let r: ZyraAgentRun
      try {
        r = await streamZyraAgent(q, (st) => setLive((prev) => [...prev, st]))
      } catch (e) {
        if (!(e instanceof AgentStreamUnsupported)) throw e
        r = await runZyraAgent(q) // older controller: no live steps, same answer
      }
      setRun(r)
      if (r.proposed_action_ids.length > 0) onProposed?.()
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="rounded-2xl border border-[color-mix(in_srgb,#0a84ff_30%,transparent)] bg-[linear-gradient(135deg,color-mix(in_srgb,#0a84ff_7%,transparent),color-mix(in_srgb,#bf5af2_5%,transparent))] p-4" data-testid="zyra-agent-panel">
      <p className="flex items-center gap-2 font-medium text-[var(--text-primary)]"><Sparkles className="h-4 w-4 text-[#0a84ff]" /> Ask Zyra to take care of something</p>
      <p className="mt-1 text-xs text-[var(--text-secondary)]">Zyra looks at your real machines and hosts, then queues what it recommends below. It never changes anything on its own.</p>

      <form className="mt-3 flex flex-wrap gap-2" onSubmit={(e) => { e.preventDefault(); void go(prompt) }}>
        <input
          className="input min-w-0 flex-1 text-sm"
          placeholder="e.g. Back up every running machine that has no backup"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          aria-label="What should Zyra look into?"
          maxLength={500}
        />
        <button type="submit" className="btn-primary text-sm" disabled={busy || !prompt.trim()}>{busy ? 'Working…' : 'Ask Zyra'}</button>
      </form>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {SUGGESTIONS.map((s) => (
          <button key={s} type="button" className="rounded-full bg-[var(--apple-fill-tertiary)]/60 px-3 py-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]" disabled={busy} onClick={() => { setPrompt(s); void go(s) }}>{s}</button>
        ))}
      </div>

      {busy ? (
        <div className="mt-3 space-y-1" data-testid="zyra-agent-live" aria-live="polite">
          <p className="flex items-center gap-2 text-sm text-[var(--text-secondary)]"><Loader2 className="h-4 w-4 animate-spin" /> {live.length === 0 ? 'Looking at your fleet…' : 'Working on it…'}</p>
          {live.filter((s) => s.kind === 'tool_call').map((s, i) => (
            <p key={i} className="flex items-start gap-1.5 pl-6 text-xs text-[var(--text-muted)]"><Wrench className="mt-0.5 h-3 w-3 shrink-0" aria-hidden /> {TOOL_LABELS[s.tool ?? ''] ?? s.tool}</p>
          ))}
        </div>
      ) : null}
      {error ? <p className="mt-3 text-xs text-red-500" role="alert">{error}</p> : null}

      {run ? (
        <div className="mt-3 space-y-3" data-testid="zyra-agent-result">
          <p className="whitespace-pre-wrap text-sm text-[var(--text-primary)]">{run.answer}</p>
          {run.proposed_action_ids.length > 0 ? (
            <p className="text-xs text-emerald-600">{run.proposed_action_ids.length} proposal{run.proposed_action_ids.length === 1 ? '' : 's'} added to the queue below — review and approve what you agree with.</p>
          ) : null}
          {run.steps.some((s) => s.kind === 'tool_call') ? (
            <details className="text-xs text-[var(--text-muted)]">
              <summary className="cursor-pointer select-none">How I got there ({run.steps.filter((s) => s.kind === 'tool_call').length} steps)</summary>
              <ul className="mt-2 space-y-1">
                {run.steps.filter((s) => s.kind === 'tool_call').map((s, i) => (
                  <li key={i} className="flex items-start gap-1.5"><Wrench className="mt-0.5 h-3 w-3 shrink-0" aria-hidden /> {TOOL_LABELS[s.tool ?? ''] ?? s.tool}<span className="font-mono opacity-70">{s.detail !== '{}' ? ` ${s.detail}` : ''}</span></li>
                ))}
              </ul>
            </details>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}
