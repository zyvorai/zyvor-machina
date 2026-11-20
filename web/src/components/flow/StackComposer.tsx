// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Loader2, Sparkles } from 'lucide-react'
import { executeZyraAction, type ZyraActionRow } from '../../api/ai'
import { draftStack, planStack, proposeStack, type StackPlan, type StackTemplate } from '../../api/stacks'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import StackPlanPreview from './StackPlan'

const EXAMPLE =
  '3 web servers behind a load balancer, an API, and a highly available postgres database with daily backups'

/** Plain English to a planned stack, proposed for approval and deployed. */
export default function StackComposer({ onDeployed }: { onDeployed?: () => void }) {
  const toast = useToastContext()
  const [name, setName] = useState('')
  const [prompt, setPrompt] = useState('')
  const [busy, setBusy] = useState<'draft' | 'plan' | 'propose' | 'approve' | null>(null)
  const [template, setTemplate] = useState<StackTemplate | null>(null)
  const [templateText, setTemplateText] = useState('')
  const [plan, setPlan] = useState<StackPlan | null>(null)
  const [source, setSource] = useState<'llm' | 'rules' | null>(null)
  const [notes, setNotes] = useState<string[]>([])
  const [action, setAction] = useState<ZyraActionRow | null>(null)

  const show = (t: StackTemplate, p: StackPlan) => {
    setTemplate(t)
    setTemplateText(JSON.stringify(t, null, 2))
    setPlan(p)
    setAction(null)
  }

  const draft = async () => {
    if (!name.trim() || !prompt.trim()) {
      toast.warning('Give the stack a name and describe it')
      return
    }
    setBusy('draft')
    try {
      const d = await draftStack({ name: name.trim(), prompt: prompt.trim() })
      setSource(d.source)
      setNotes(d.notes)
      show(d.template, d.plan)
    } catch (e: unknown) {
      toast.error(`Draft failed: ${formatUserError(e)}`)
    } finally {
      setBusy(null)
    }
  }

  const replan = async () => {
    let t: StackTemplate
    try {
      t = JSON.parse(templateText) as StackTemplate
    } catch {
      toast.error('Template must be valid JSON')
      return
    }
    setBusy('plan')
    try {
      show(t, await planStack({ name: name.trim(), template: t }))
    } catch (e: unknown) {
      toast.error(`Plan failed: ${formatUserError(e)}`)
    } finally {
      setBusy(null)
    }
  }

  const propose = async () => {
    if (!template) return
    setBusy('propose')
    try {
      setAction(await proposeStack({ name: name.trim(), template, prompt: prompt.trim() || undefined }))
      toast.success('Sent for approval')
    } catch (e: unknown) {
      toast.error(`Propose failed: ${formatUserError(e)}`)
    } finally {
      setBusy(null)
    }
  }

  const approve = async () => {
    if (!action) return
    setBusy('approve')
    try {
      await executeZyraAction(action.id)
      toast.success(`Deploying stack ${name.trim()}`)
      setAction(null)
      setPlan(null)
      setTemplate(null)
      setPrompt('')
      setName('')
      onDeployed?.()
    } catch (e: unknown) {
      toast.error(`Approve failed: ${formatUserError(e)}`)
    } finally {
      setBusy(null)
    }
  }

  const blocked = !!plan && (plan.errors.length > 0 || !plan.quota.ok || !plan.placement.ok)

  return (
    <section
      aria-label="Compose stack"
      className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3"
    >
      <h2 className="text-sm font-medium text-[var(--text-secondary)] flex items-center gap-2">
        <Sparkles className="w-4 h-4 text-[var(--accent)]" /> Describe a stack
      </h2>
      <input
        aria-label="New stack name"
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="Stack name, e.g. shop"
        className="w-full max-w-md input-field text-sm"
      />
      <textarea
        aria-label="Describe the stack"
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        placeholder={EXAMPLE}
        rows={3}
        className="w-full input-field text-sm"
      />
      <button type="button" className="btn-primary text-sm inline-flex items-center gap-1.5" disabled={!!busy} onClick={() => void draft()}>
        {busy === 'draft' && <Loader2 className="w-4 h-4 animate-spin" />} Draft plan
      </button>

      {plan && (
        <div className="space-y-4 border-t border-[var(--apple-hairline)] pt-4">
          <p className="text-xs text-[var(--text-muted)]">
            {source === 'llm' ? 'Drafted by Zyvor.' : 'Drafted from rules (no LLM configured, or its draft was unusable).'}
            {notes.map((n) => (
              <span key={n} className="block">{n}</span>
            ))}
          </p>
          <StackPlanPreview plan={plan} />
          <details>
            <summary className="text-xs text-[var(--text-muted)] cursor-pointer">Edit template</summary>
            <textarea
              aria-label="Template (JSON)"
              value={templateText}
              onChange={(e) => setTemplateText(e.target.value)}
              rows={14}
              className="mt-2 w-full font-mono text-xs input-field"
            />
            <button type="button" className="btn-secondary text-sm mt-2" disabled={!!busy} onClick={() => void replan()}>
              Plan again
            </button>
          </details>
          {action ? (
            <div className="flex flex-wrap items-center gap-3" role="status" aria-label="Approval">
              <span className="text-sm">Waiting for approval: {action.review}</span>
              <button type="button" className="btn-primary text-sm" disabled={!!busy} onClick={() => void approve()}>
                Approve and deploy
              </button>
            </div>
          ) : (
            <button
              type="button"
              className="btn-primary text-sm"
              disabled={!!busy || blocked}
              title={blocked ? 'Fix the problems above first' : undefined}
              onClick={() => void propose()}
            >
              Propose for approval
            </button>
          )}
        </div>
      )}
    </section>
  )
}
