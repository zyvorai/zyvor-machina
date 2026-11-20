// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { MacSheet } from './mac/PlatformMacUi'
import { invokeVmLibvirt, queryVmLibvirt, type CpuMemoryTopology } from '../../api/platformVmLibvirt'
import { formatUserError } from '../../utils/apiError'

type Props = {
  open: boolean
  vmId: string
  vmName: string
  onClose: () => void
  onSaved: () => void
  onNotify: (msg: string) => void
  onError: (msg: string) => void
}

export default function VmCpuTopologyModal({ open, vmId, vmName, onClose, onSaved, onNotify, onError }: Props) {
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [sockets, setSockets] = useState('1')
  const [cores, setCores] = useState('1')
  const [threads, setThreads] = useState('1')
  const [vcpus, setVcpus] = useState(1)
  const [state, setState] = useState('')

  useEffect(() => {
    if (!open || !vmId) return
    setLoading(true)
    void queryVmLibvirt<CpuMemoryTopology>(vmId, 'cpu.memory.topology')
      .then((t) => {
        setSockets(String(t.sockets))
        setCores(String(t.cores))
        setThreads(String(t.threads))
        setVcpus(t.vcpus)
        setState(t.state)
      })
      .catch((e: unknown) => onError(formatUserError(e)))
      .finally(() => setLoading(false))
  }, [open, vmId, onError])

  const computed = Math.max(1, Number(sockets) || 1) * Math.max(1, Number(cores) || 1) * Math.max(1, Number(threads) || 1)

  const save = async () => {
    setSaving(true)
    try {
      await invokeVmLibvirt(vmId, 'cpu.topology.set', {
        sockets: Number(sockets),
        cores: Number(cores),
        threads: Number(threads),
      })
      onNotify(`CPU topology updated for ${vmName}`)
      onSaved()
      onClose()
    } catch (e: unknown) {
      onError(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <MacSheet open={open} onClose={onClose} title={`CPU topology — ${vmName}`} ariaLabel="CPU topology">
      {loading ? (
        <p className="text-sm text-[var(--text-muted)]">Loading topology…</p>
      ) : (
        <div className="space-y-4">
          <p className="text-xs text-[var(--text-muted)]">
            Guest state: <span className="text-[var(--text-secondary)] capitalize">{state || '—'}</span>
            {' · '}
            Active vCPUs: <span className="text-[var(--text-secondary)]">{vcpus}</span>
          </p>
          <div className="grid grid-cols-3 gap-3">
            <label className="block text-sm">
              <span className="text-[var(--text-muted)]">Sockets</span>
              <input type="number" min={1} className="input mt-1 w-full" value={sockets} onChange={(e) => setSockets(e.target.value)} />
            </label>
            <label className="block text-sm">
              <span className="text-[var(--text-muted)]">Cores</span>
              <input type="number" min={1} className="input mt-1 w-full" value={cores} onChange={(e) => setCores(e.target.value)} />
            </label>
            <label className="block text-sm">
              <span className="text-[var(--text-muted)]">Threads</span>
              <input type="number" min={1} className="input mt-1 w-full" value={threads} onChange={(e) => setThreads(e.target.value)} />
            </label>
          </div>
          <p className="text-xs text-[var(--text-muted)]">
            Persistent vCPUs: <span className="text-[var(--text-primary)]">{computed}</span>
            {computed !== vcpus && state === 'running' && (
              <span className="text-amber-600/90"> — needs shutdown to apply topology change</span>
            )}
          </p>
          <button type="button" className="btn-primary text-sm w-full" disabled={saving} onClick={() => void save()}>
            {saving ? 'Saving…' : 'Save topology'}
          </button>
        </div>
      )}
    </MacSheet>
  )
}
