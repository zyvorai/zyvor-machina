// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { CheckCircle2, History, ShieldCheck, Undo2 } from 'lucide-react'
import ConfirmDialog from '../../components/ConfirmDialog'
import PlatformPageChrome, { PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import ZyraInsightCard from '../../components/ai/ZyraInsightCard'
import ZyraAgentPanel from '../../components/ai/ZyraAgentPanel'
import ZyraTrustLadder from '../../components/ai/ZyraTrustLadder'
import FleetDigestPanel from '../../components/ai/FleetDigestPanel'
import ChangeReviewPanel from '../../components/ai/ChangeReviewPanel'
import {
  executeZyraAction,
  getZyraApprovalHub,
  listZyraActionHistory,
  rejectZyraAction,
  undoZyraAction,
  verifyZyraAction,
  type ZyraActionHistoryRow,
  type ZyraActionRow,
} from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export default function PlatformZyraApprovals() {
  const toast = useToastContext()
  const [actions, setActions] = useState<ZyraActionRow[]>([])
  const [totalPending, setTotalPending] = useState(0)
  const [firewallPending, setFirewallPending] = useState(0)
  const [busy, setBusy] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [rejectTargetId, setRejectTargetId] = useState<string | null>(null)
  const [confirmExecute, setConfirmExecute] = useState<ZyraActionRow | null>(null)
  const [history, setHistory] = useState<ZyraActionHistoryRow[]>([])
  const [undoTarget, setUndoTarget] = useState<ZyraActionHistoryRow | null>(null)

  // `quiet` refreshes in place (no full-page spinner) so panels with local state keep what they show.
  const load = useCallback(async (quiet = false) => {
    if (!quiet) setLoading(true)
    setError(null)
    try {
      const hub = await getZyraApprovalHub()
      setActions(hub.zyra_actions ?? [])
      setTotalPending(hub.total_pending)
      setFirewallPending(hub.firewall_pending)
      // History is a nice-to-have: an older controller without the route must not break the queue.
      setHistory(await listZyraActionHistory().catch(() => []))
    } catch (e: unknown) {
      setError(formatUserError(e))
      setActions([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const execute = async (id: string) => {
    setBusy(id)
    try {
      const r = await executeZyraAction(id)
      toast.success(String(r.message ?? 'Executed'))
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const doReject = async (id: string) => {
    setRejectTargetId(null)
    setBusy(id)
    try {
      await rejectZyraAction(id)
      toast.success('Action rejected')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const verify = async (id: string) => {
    setBusy(id)
    try {
      const r = await verifyZyraAction(id)
      toast.success(r.detail || `Check: ${r.status}`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const undo = async (id: string) => {
    setUndoTarget(null)
    setBusy(id)
    try {
      const r = await undoZyraAction(id)
      toast.success(r.message ?? 'Undone')
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      title="Zyra approvals"
      subtitle="Review and execute pending AI actions from Zeus, NL Ops, and firewall automation"
      icon={<ShieldCheck className="w-6 h-6 text-orange-400" />}
      loading={loading}
      error={error}
      onErrorRetry={() => void load()}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <ZyraAgentPanel onProposed={() => void load(true)} />

      <MacGlassPanel
        title="Pending queue"
        subtitle={`${totalPending} total · ${firewallPending} firewall-related`}
      >
        {actions.length === 0 && !loading && (
          <PlatformEmptyState
            title="No pending approvals"
            subtitle="When Zyra or NL Ops queues a risky change, it appears here for human approval."
          />
        )}
        <div className="space-y-2">
          {actions.map((a) => (
            <div key={a.id}>
              <ZyraInsightCard
                title={a.label}
                detail={`${a.review} · Risk: ${a.risk} · ${a.source}`}
                onApprove={busy === a.id ? undefined : () => setConfirmExecute(a)}
                onDismiss={busy === a.id ? undefined : () => setRejectTargetId(a.id)}
              />
              <ChangeReviewPanel actionId={a.id} />
            </div>
          ))}
        </div>
      </MacGlassPanel>

      <FleetDigestPanel />

      <ZyraTrustLadder />

      {history.length > 0 && (
        <MacGlassPanel title="Recent actions" subtitle="What Zyra and your team executed — check it worked, or undo it when it is reversible">
          <ul className="divide-y divide-[var(--apple-hairline)]" data-testid="zyra-action-history">
            {history.map((h) => {
              const v = h.verify?.status
              return (
                <li key={h.id} className="flex flex-wrap items-center gap-3 py-3">
                  <History className="h-4 w-4 shrink-0 text-[var(--text-muted)]" aria-hidden />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-[var(--text-primary)]">{h.label}</p>
                    <p className="text-xs text-[var(--text-muted)]">
                      {h.status === 'failed' ? 'Failed' : h.undone_at ? 'Undone' : 'Executed'}
                      {h.approved_by ? ` · approved by ${h.approved_by}` : ''}
                      {h.executed_at ? ` · ${new Date(h.executed_at.replace(' ', 'T') + (h.executed_at.endsWith('Z') ? '' : 'Z')).toLocaleString()}` : ''}
                    </p>
                    {h.verify?.detail ? (
                      <p className={`text-xs ${v === 'ok' ? 'text-emerald-600' : v === 'failed' ? 'text-red-500' : 'text-[var(--text-muted)]'}`}>
                        {v === 'ok' ? '✓ ' : v === 'failed' ? '✕ ' : ''}{h.verify.detail}
                      </p>
                    ) : null}
                  </div>
                  <div className="flex items-center gap-2">
                    {h.status === 'executed' && !h.undone_at ? (
                      <button type="button" className="btn-secondary text-xs" disabled={busy === h.id} onClick={() => void verify(h.id)}>Check it worked</button>
                    ) : null}
                    {h.undoable ? (
                      <button type="button" className="btn-secondary inline-flex items-center gap-1 text-xs" disabled={busy === h.id} onClick={() => setUndoTarget(h)}>
                        <Undo2 className="h-3.5 w-3.5" /> Undo
                      </button>
                    ) : null}
                  </div>
                </li>
              )
            })}
          </ul>
        </MacGlassPanel>
      )}

      {actions.length > 0 && (
        <p className="text-xs text-[var(--text-muted)] flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400/80" />
          Executed actions are audited on the controller — reject to discard without side effects.
        </p>
      )}
      <ConfirmDialog
        open={rejectTargetId !== null}
        title="Reject AI Action"
        message="Reject this AI action? It will not be executed."
        confirmLabel="Reject"
        variant="danger"
        onCancel={() => setRejectTargetId(null)}
        onConfirm={() => { if (rejectTargetId) void doReject(rejectTargetId) }}
      />
      <ConfirmDialog
        open={undoTarget !== null}
        title="Undo this action"
        message={undoTarget ? `Undo "${undoTarget.label}"? This puts the machine back as it was before the action.` : ''}
        confirmLabel="Undo"
        variant="danger"
        onCancel={() => setUndoTarget(null)}
        onConfirm={() => { if (undoTarget) void undo(undoTarget.id) }}
      />
      <ConfirmDialog
        open={confirmExecute !== null}
        title="Approve Zyra action"
        message={confirmExecute ? `Approve and execute "${confirmExecute.label}"? ${confirmExecute.review} · Risk: ${confirmExecute.risk}` : ''}
        confirmLabel="Approve"
        variant="danger"
        onCancel={() => setConfirmExecute(null)}
        onConfirm={() => {
          const a = confirmExecute
          setConfirmExecute(null)
          if (a) void execute(a.id)
        }}
      />
    </PlatformPageChrome>
  )
}
