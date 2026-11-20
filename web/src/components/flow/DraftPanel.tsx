// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Plain English → policy YAML. The draft is validated and replayed against
// the flow history; nothing is applied. On the fleet, Zyvor's LLM drafts
// when one is configured and a draft goes to a second admin for approval.

import { useCallback, useEffect, useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../platform/tahoe/TahoeListKit'
import { headRowCls, rowCls, thCls } from '../bpf/shared'
import {
  DRAFT_EXAMPLES,
  approveJit,
  draftVmNetpol,
  listDraftPending,
  proposeVmNetpol,
  rejectJit,
  type DraftPending,
  type NetpolDraft,
  type NetpolScope,
} from '../../api/vmNetpol'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { statusPillClasses, statusToneClass } from '../../utils/semanticColors'

export default function DraftPanel({
  scope,
  editorYaml,
  onDraft,
  onChanged,
}: {
  scope: NetpolScope
  /** The YAML in the editor now, which is what gets sent for approval. */
  editorYaml: string
  onDraft: (d: NetpolDraft) => void
  onChanged: () => void
}) {
  const toast = useToastContext()
  const fleet = scope === 'fleet'
  const [prompt, setPrompt] = useState('')
  const [draft, setDraft] = useState<NetpolDraft | null>(null)
  const [pending, setPending] = useState<DraftPending[]>([])
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    if (!fleet) return
    try {
      setPending(await listDraftPending())
    } catch {
      setPending([])
    }
  }, [fleet])

  useEffect(() => { void load() }, [load])

  const run = async () => {
    setBusy(true)
    try {
      const d = await draftVmNetpol(scope, prompt.trim())
      setDraft(d)
      onDraft(d)
    } catch (e: unknown) {
      setDraft(null)
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const propose = async () => {
    setBusy(true)
    try {
      const r = await proposeVmNetpol(editorYaml, prompt.trim())
      toast.success(`Requested: ${r.pending.label} — another admin approves it below or in Approvals`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const decide = async (p: DraftPending, approve: boolean) => {
    try {
      await (approve ? approveJit(p.id) : rejectJit(p.id))
      toast.success(approve ? `Approved and applied: ${p.label}` : `Rejected: ${p.label}`)
      await load()
      if (approve) onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const breaking = draft?.replay.would_break.length ?? 0

  return (
    <MacGlassPanel
      title="Describe in plain English"
      subtitle={`Write what should be allowed or blocked; the draft lands in the editor below, validated and replayed against recorded flows. Nothing is applied until you ${fleet ? 'request approval and another admin approves' : 'press Apply'}.`}
    >
      <textarea
        aria-label="Policy in plain English"
        className="input w-full h-20 text-sm"
        placeholder={DRAFT_EXAMPLES.join('\n')}
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
      />
      <div className="flex flex-wrap items-center gap-2 mt-2">
        <button type="button" className="btn-primary text-sm" disabled={busy || prompt.trim() === ''} onClick={() => void run()}>
          Draft policy
        </button>
        {fleet && draft && (
          <button type="button" className="btn-secondary text-sm" disabled={busy} onClick={() => void propose()} title="File the YAML in the editor for another admin to approve">
            Request approval
          </button>
        )}
        {DRAFT_EXAMPLES.slice(0, 3).map((x) => (
          <button key={x} type="button" className="text-xs text-[var(--accent,#0071e3)] hover:underline" onClick={() => setPrompt(x)}>
            {x}
          </button>
        ))}
      </div>

      {draft && (
        <div className="mt-3 space-y-1 text-xs" aria-label="Draft notes">
          <div className="flex flex-wrap items-center gap-2">
            <span className={statusPillClasses(draft.preview.valid ? 'ok' : 'error')}>{draft.preview.valid ? 'Valid' : 'Invalid'}</span>
            <span className="text-[var(--text-muted)]">Drafted by {draft.source === 'llm' ? 'Zyvor' : 'the sentence parser'}</span>
            <span className={statusPillClasses(breaking > 0 ? 'warn' : 'ok')}>
              {breaking > 0 ? `Would break ${breaking} connection(s)` : `Safe for ${draft.replay.evaluated} recorded connection(s)`}
            </span>
          </div>
          {draft.notes.map((n) => <div key={n}>{n}</div>)}
          {draft.unparsed.map((u) => <div key={u} className={statusToneClass('warn')}>Not used: {u}</div>)}
        </div>
      )}

      {pending.length > 0 && (
        <div className="mt-4">
          <TahoeTableWrap>
            <table className="w-full text-xs" aria-label="Drafts waiting for approval">
              <thead>
                <tr className={headRowCls}>
                  <th scope="col" className={thCls}>Draft</th>
                  <th scope="col" className={thCls}>Asked for</th>
                  <th scope="col" className={thCls}>By</th>
                  <th scope="col" className={thCls}>Since</th>
                  <th scope="col" className="py-2"><span className="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                {pending.map((p) => (
                  <tr key={p.id} className={rowCls}>
                    <td className="py-2 pr-2 font-medium">{p.label}</td>
                    <td className="py-2 pr-2">{p.object_ref?.prompt || '—'}</td>
                    <td className="py-2 pr-2">{p.requested_by}</td>
                    <td className="py-2 pr-2 text-[var(--text-muted)]">{p.created_at}</td>
                    <td className="py-2 text-right whitespace-nowrap">
                      <button type="button" className="btn-primary text-xs mr-2" onClick={() => void decide(p, true)}>Approve</button>
                      <button type="button" className="btn-secondary text-xs" onClick={() => void decide(p, false)}>Reject</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TahoeTableWrap>
        </div>
      )}
    </MacGlassPanel>
  )
}
