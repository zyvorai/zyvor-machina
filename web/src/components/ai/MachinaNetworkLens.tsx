// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState } from 'react'
import { Network } from 'lucide-react'
import { aiNetworkExplain } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { statusToneClass } from '../../utils/semanticColors'

export default function MachinaNetworkLens({ vmNames = [] }: { vmNames?: string[] }) {
  const [vmA, setVmA] = useState(vmNames[0] ?? '')
  const [vmB, setVmB] = useState(vmNames[1] ?? '')
  const [port, setPort] = useState('')
  const [result, setResult] = useState<{ can_reach: boolean; explanation: string; hops: string[]; remediation: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const analyze = async () => {
    if (!vmA.trim() || !vmB.trim()) return
    setBusy(true)
    setError(null)
    try {
      const p = port.trim() ? Number(port) : undefined
      setResult(await aiNetworkExplain(vmA.trim(), vmB.trim(), p))
    } catch (e: unknown) {
      setError(formatUserError(e))
      setResult(null)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rounded-2xl border border-white/[0.06] bg-[var(--apple-surface)] p-4 space-y-3">
      <h3 className="font-semibold text-sm flex items-center gap-2">
        <Network className={`w-4 h-4 ${statusToneClass('info')}`} /> Machina Network Lens
      </h3>
      <p className="text-xs text-[var(--text-muted)]">Rule-based path analysis — why can&apos;t A reach B?</p>
      <div className="flex flex-wrap gap-2 items-end">
        <label className="text-xs">Source VM<input className="input block mt-1 w-36" value={vmA} onChange={(e) => setVmA(e.target.value)} list="network-lens-vms" /></label>
        <label className="text-xs">Target VM<input className="input block mt-1 w-36" value={vmB} onChange={(e) => setVmB(e.target.value)} list="network-lens-vms" /></label>
        <label className="text-xs">Port<input className="input block mt-1 w-20" value={port} onChange={(e) => setPort(e.target.value)} placeholder="443" /></label>
        <button type="button" className="btn-primary text-xs" disabled={busy} onClick={() => void analyze()}>{busy ? 'Analyzing…' : 'Analyze path'}</button>
      </div>
      {vmNames.length > 0 && (
        <datalist id="network-lens-vms">{vmNames.map((n) => <option key={n} value={n} />)}</datalist>
      )}
      {error && <p className={`text-xs ${statusToneClass('error')}`}>{error}</p>}
      {result && (
        <div className="text-sm space-y-2 border-t border-white/[0.06] pt-3">
          <p className={result.can_reach ? statusToneClass('ok') : statusToneClass('warn')}>
            {result.can_reach ? 'Likely reachable' : 'Blocked or unknown'}
          </p>
          <p className="text-[var(--text-secondary)]">{result.explanation}</p>
          <p className="text-xs text-[var(--text-muted)]">Path: {result.hops.join(' → ')}</p>
          <p className="text-xs text-[var(--text-muted)]">Fix: {result.remediation}</p>
        </div>
      )}
    </div>
  )
}
