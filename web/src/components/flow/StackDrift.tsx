// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { executeZyraAction, type ZyraActionRow } from '../../api/ai'
import {
  convergeStack,
  getStackDrift,
  planStack,
  proposeStack,
  setStackAutoHeal,
  updateStack,
  type NativeStack,
  type StackDrift,
  type StackPlan,
  type StackTemplate,
} from '../../api/stacks'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import StackPlanPreview, { DriftBadge } from './StackPlan'

export function StackDriftPanel({ stack, onChanged }: { stack: NativeStack; onChanged: () => void }) {
  const toast = useToastContext()
  const [drift, setDrift] = useState<StackDrift | null>(stack.drift_json ?? null)
  const [busy, setBusy] = useState(false)
  const [autoHeal, setAutoHeal] = useState(!!stack.auto_heal)

  const run = async (fn: () => Promise<StackDrift>, done: string) => {
    setBusy(true)
    try {
      setDrift(await fn())
      toast.success(done)
      onChanged()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const items = drift?.items ?? []
  return (
    <section aria-label="Stack drift" className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4 space-y-3 text-sm">
      <div className="flex flex-wrap items-center gap-3">
        <DriftBadge stack={{ ...stack, drift_json: drift ?? undefined }} />
        <span className="text-[var(--text-muted)]">
          {stack.checked_at ? `Checked ${new Date(stack.checked_at).toLocaleString()}` : 'Not checked yet'}
        </span>
        <button type="button" className="btn-secondary text-sm" disabled={busy} onClick={() => void run(() => getStackDrift(stack.id), 'Checked')}>
          Check now
        </button>
        <button type="button" className="btn-primary text-sm" disabled={busy} onClick={() => void run(() => convergeStack(stack.id), 'Converged')}>
          Converge now
        </button>
        <label className="inline-flex items-center gap-2">
          <input
            type="checkbox"
            aria-label="Auto-heal"
            checked={autoHeal}
            disabled={busy}
            onChange={async (e) => {
              const on = e.target.checked
              setAutoHeal(on)
              try {
                await setStackAutoHeal(stack.id, on)
              } catch (err: unknown) {
                setAutoHeal(!on)
                toast.error(formatUserError(err))
              }
            }}
          />
          Auto-heal (recreate missing VMs, re-apply policies)
        </label>
      </div>
      {items.length === 0 ? (
        <p className="text-[var(--text-muted)]">Nothing differs from the template.</p>
      ) : (
        <table className="apple-table" aria-label="Drift">
          <thead>
            <tr>
              <th scope="col">Kind</th>
              <th scope="col">Name</th>
              <th scope="col">What</th>
              <th scope="col">Fixed</th>
            </tr>
          </thead>
          <tbody>
            {items.map((i) => (
              <tr key={`${i.kind}:${i.name}:${i.detail}`}>
                <td>{i.kind}</td>
                <td className="font-mono text-xs">{i.name}</td>
                <td>{i.detail}</td>
                <td>{i.fixed ? 'Yes' : 'No'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  )
}

export function StackTemplateEditor({ stack, onChanged }: { stack: NativeStack; onChanged: () => void }) {
  const toast = useToastContext()
  const [text, setText] = useState(JSON.stringify(stack.template_json, null, 2))
  const [plan, setPlan] = useState<StackPlan | null>(null)
  const [planned, setPlanned] = useState<StackTemplate | null>(null)
  const [action, setAction] = useState<ZyraActionRow | null>(null)
  const [busy, setBusy] = useState(false)

  const guard = async (fn: () => Promise<void>) => {
    setBusy(true)
    try {
      await fn()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const doPlan = () =>
    guard(async () => {
      const t = JSON.parse(text) as StackTemplate
      setPlan(await planStack({ name: stack.name, template: t, stack_id: stack.id }))
      setPlanned(t)
      setAction(null)
    })

  const blocked = !!plan && (plan.errors.length > 0 || !plan.quota.ok || !plan.placement.ok)
  return (
    <section aria-label="Stack template" className="space-y-3">
      <textarea
        aria-label="Template (JSON)"
        value={text}
        onChange={(e) => {
          setText(e.target.value)
          setPlan(null)
        }}
        rows={16}
        className="w-full font-mono text-xs input-field"
      />
      <button type="button" className="btn-secondary text-sm" disabled={busy} onClick={() => void doPlan()}>
        Plan update
      </button>
      {plan && planned && (
        <div className="space-y-3">
          <StackPlanPreview plan={plan} />
          <div className="flex flex-wrap gap-2 items-center">
            {action ? (
              <>
                <span className="text-sm" role="status">Waiting for approval: {action.review}</span>
                <button
                  type="button"
                  className="btn-primary text-sm"
                  disabled={busy}
                  onClick={() =>
                    void guard(async () => {
                      await executeZyraAction(action.id)
                      toast.success('Updating stack')
                      setAction(null)
                      setPlan(null)
                      onChanged()
                    })
                  }
                >
                  Approve and apply
                </button>
              </>
            ) : (
              <>
                <button
                  type="button"
                  className="btn-primary text-sm"
                  disabled={busy || blocked}
                  onClick={() =>
                    void guard(async () => {
                      setAction(await proposeStack({ name: stack.name, template: planned, stack_id: stack.id }))
                      toast.success('Sent for approval')
                    })
                  }
                >
                  Propose update
                </button>
                <button
                  type="button"
                  className="btn-secondary text-sm"
                  disabled={busy || blocked}
                  onClick={() =>
                    void guard(async () => {
                      await updateStack(stack.id, planned)
                      toast.success('Stack updated')
                      setPlan(null)
                      onChanged()
                    })
                  }
                >
                  Apply now
                </button>
              </>
            )}
          </div>
        </div>
      )}
    </section>
  )
}
