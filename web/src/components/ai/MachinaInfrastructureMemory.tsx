// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Clock, History } from 'lucide-react'
import ConfirmDialog from '../ConfirmDialog'
import { Link } from 'react-router'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import {
  getInfrastructureMemory,
  getMemoryChangesBefore,
  purgeMemory,
  type InfrastructureMemory,
} from '../../api/ai'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusToneClass } from '../../utils/semanticColors'

export default function MachinaInfrastructureMemory() {
  const toast = useToastContext()
  const [memory, setMemory] = useState<InfrastructureMemory | null>(null)
  const [purging, setPurging] = useState(false)
  const [hoursBefore, setHoursBefore] = useState(4)
  const [changesSummary, setChangesSummary] = useState<string | null>(null)
  const [changes, setChanges] = useState<Array<{ at: string; kind: string; summary: string; actor: string }>>([])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [showClearConfirm, setShowClearConfirm] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    try {
      setMemory(await getInfrastructureMemory(30))
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const loadChanges = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await getMemoryChangesBefore({ hours_before: hoursBefore })
      setChangesSummary(r.summary)
      setChanges(r.changes ?? [])
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const doClearMemory = async () => {
    setShowClearConfirm(false)
    setPurging(true)
    try {
      const r = await purgeMemory('all')
      toast.success(`Cleared ${r.deleted} entries`)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setPurging(false)
    }
  }

  return (
    <div className="space-y-4">
      <ConfirmDialog
        open={showClearConfirm}
        title="Clear Infrastructure Memory"
        message="Clear all infrastructure memory entries? Persisted incidents and lessons from RCA will be permanently deleted."
        confirmLabel="Clear all"
        variant="danger"
        onCancel={() => setShowClearConfirm(false)}
        onConfirm={() => void doClearMemory()}
      />
      <MacGlassPanel
        title="Infrastructure memory"
        subtitle="Persisted incidents and lessons from RCA"
        action={
          <button
            type="button"
            className="btn-destructive text-xs"
            disabled={purging}
            onClick={() => setShowClearConfirm(true)}
          >
            {purging ? 'Clearing…' : 'Clear memory'}
          </button>
        }
      >
        {error && <p className={`text-sm ${statusToneClass('error')}`}>{error}</p>}
        <p className="text-sm text-[var(--text-muted)]">
          {memory?.incidents.length ?? 0} remembered incident(s)
          {memory?.runbook_hints?.[0] ? ` · ${memory.runbook_hints[0]}` : ''}
        </p>
        <ul className="mt-3 space-y-2 text-xs max-h-56 overflow-y-auto">
          {(memory?.incidents ?? []).map((inc, i) => (
            <li key={`${inc.at}-${i}`} className="rounded-lg border border-white/[0.06] p-2">
              <p className="font-medium text-[var(--text-primary)] flex items-center gap-1">
                <Clock className="w-3 h-3 opacity-60" />
                {new Date(inc.at).toLocaleString()} · {inc.kind}
              </p>
              <p className="text-[var(--text-muted)] mt-0.5">{inc.summary}</p>
              <p className="text-[var(--text-muted)] mt-0.5">{inc.lesson}</p>
            </li>
          ))}
          {(memory?.incidents.length ?? 0) === 0 && (
            <li className="text-[var(--text-muted)]">No incidents in memory yet — POST RCA analysis to persist.</li>
          )}
        </ul>
        <Link to="/platform/zyra/incidents" className={`text-xs mt-3 inline-block ${hubLinkClasses()}`}>
          Open Incident Commander →
        </Link>
      </MacGlassPanel>

      <MacGlassPanel title="What changed before an outage?" subtitle="Audit delta in the pre-incident window">
        <div className="flex flex-wrap items-end gap-3 mb-3">
          <label className="text-xs text-[var(--text-muted)]">
            Hours before
            <input
              type="number"
              min={1}
              max={48}
              className="input mt-1 block w-20 text-sm"
              value={hoursBefore}
              onChange={(e) => setHoursBefore(Number(e.target.value))}
            />
          </label>
          <button type="button" className="btn-primary text-xs flex items-center gap-1" disabled={busy} onClick={() => void loadChanges()}>
            <History className="w-3.5 h-3.5" />
            {busy ? 'Loading…' : 'Query audit delta'}
          </button>
        </div>
        {changesSummary && <p className="text-sm text-[var(--text-secondary)]">{changesSummary}</p>}
        <ul className="mt-2 space-y-1 text-xs font-mono max-h-40 overflow-y-auto">
          {changes.map((c, i) => (
            <li key={`${c.at}-${i}`} className="text-[var(--text-muted)]">
              [{c.kind}] {c.summary} — {c.actor}
            </li>
          ))}
        </ul>
      </MacGlassPanel>
    </div>
  )
}
