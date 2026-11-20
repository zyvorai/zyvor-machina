// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Learn mode: least-privilege policies generated from the observed flow
// history, replayed against that history before anything is applied.

import { useState } from 'react'
import { MacGlassPanel } from '../platform/mac/PlatformMacUi'
import { Empty, Field } from '../bpf/shared'
import ReplaySummary from './ReplaySummary'
import { learnVmNetpol, replayVmNetpol, type LearnResult, type NetpolScope, type ReplayResult } from '../../api/vmNetpol'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import { statusToneClass } from '../../utils/semanticColors'

export default function LearnPanel({
  scope,
  vmNames,
  onOpenInEditor,
}: {
  scope: NetpolScope
  vmNames: string[]
  onOpenInEditor: (yaml: string) => void
}) {
  const toast = useToastContext()
  const [vm, setVm] = useState('')
  const [groupBy, setGroupBy] = useState('app')
  const [minCount, setMinCount] = useState('1')
  const [l7, setL7] = useState(true)
  const [lock, setLock] = useState(false)
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<LearnResult | null>(null)
  const [replay, setReplay] = useState<ReplayResult | null>(null)

  const run = async () => {
    setBusy(true)
    setReplay(null)
    try {
      const r = await learnVmNetpol(scope, {
        vm: vm.trim() || undefined,
        group_by: groupBy.trim() || undefined,
        min_count: Number(minCount) || 1,
        l7,
        lock_unobserved: lock,
      })
      setResult(r)
      if (r.yaml.trim()) setReplay(await replayVmNetpol(scope, r.yaml))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
      <MacGlassPanel
        title="Learn policies from traffic"
        subtitle="Generates least-privilege CiliumNetworkPolicy YAML from what VMs actually did over the last 7 days — VM peers by label, external names via toFQDNs, HTTP paths, DNS names and TLS SNI. Run in observe mode first so the history is complete."
      >
        <div className="flex flex-wrap items-end gap-3">
          <Field label="VM (blank = all)" htmlFor="ln-vm">
            <input id="ln-vm" list="ln-vms" className="input text-sm w-44" value={vm} onChange={(e) => setVm(e.target.value)} />
          </Field>
          <Field label="Group VMs by label" htmlFor="ln-group">
            <input id="ln-group" className="input text-sm w-32 font-mono" value={groupBy} onChange={(e) => setGroupBy(e.target.value)} />
          </Field>
          <Field label="Min flows" htmlFor="ln-min">
            <input id="ln-min" className="input text-sm w-20" inputMode="numeric" value={minCount} onChange={(e) => setMinCount(e.target.value)} />
          </Field>
          <label className="text-xs flex items-center gap-1 pb-2">
            <input type="checkbox" checked={l7} onChange={(e) => setL7(e.target.checked)} /> L7 rules
          </label>
          <label className="text-xs flex items-center gap-1 pb-2" title="Emit default deny for a direction with no observed traffic">
            <input type="checkbox" checked={lock} onChange={(e) => setLock(e.target.checked)} /> Lock unused directions
          </label>
          <button type="button" className="btn-primary text-sm" disabled={busy} onClick={() => void run()}>
            {busy ? 'Learning…' : 'Generate'}
          </button>
          <datalist id="ln-vms">{vmNames.map((n) => <option key={n} value={n} />)}</datalist>
        </div>
        {result && (
          <div className="mt-4 space-y-3">
            <div className="text-xs text-[var(--text-muted)]">
              {result.policies.length} policies from {result.edges_used} flow edges
              {result.edges_skipped > 0 ? ` · ${result.edges_skipped} skipped (dropped by the datapath or below the minimum)` : ''}
            </div>
            {result.policies.length > 0 && (
              <ul className="text-xs space-y-1">
                {result.policies.map((p) => (
                  <li key={p.name}>
                    <span className="font-mono font-medium">{p.name}</span>
                    <span className="text-[var(--text-muted)]"> · {p.vms.join(', ')} · {p.ingress_rules} ingress, {p.egress_rules} egress rule(s)</span>
                  </li>
                ))}
              </ul>
            )}
            {result.notes.map((n) => <div key={n} className={`text-xs ${statusToneClass('warn')}`}>{n}</div>)}
            {result.yaml.trim() ? (
              <>
                <textarea
                  aria-label="Learned policy YAML"
                  readOnly
                  className="input w-full h-[360px] font-mono text-xs leading-relaxed bg-[#0b0b0d] text-[#e5e5ea]"
                  value={result.yaml}
                />
                <button type="button" className="btn-primary text-sm" onClick={() => onOpenInEditor(result.yaml)}>
                  Open in editor
                </button>
              </>
            ) : (
              <Empty>No traffic in the history for this selection yet.</Empty>
            )}
          </div>
        )}
      </MacGlassPanel>
      <MacGlassPanel title="Replay" subtitle="The learned set (on top of the current policies) replayed against every connection in the flow history.">
        {replay ? <ReplaySummary r={replay} /> : <Empty>Generate to see what the learned policies would have blocked.</Empty>}
      </MacGlassPanel>
    </div>
  )
}
