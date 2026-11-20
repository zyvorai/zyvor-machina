// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { MacSheet } from './mac/PlatformMacUi'
import { invokeVmLibvirt, queryVmLibvirt } from '../../api/platformVmLibvirt'
import { setVmMemory } from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import type { CpuMemoryTopology } from '../../api/platformVmLibvirt'

type Props = {
  open: boolean
  vmId: string
  vmName: string
  running: boolean
  onClose: () => void
  onSaved: () => void
  onNotify: (msg: string) => void
  onError: (msg: string) => void
}

export default function VmMemorySizingModal({ open, vmId, vmName, running, onClose, onSaved, onNotify, onError }: Props) {
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [currentGiB, setCurrentGiB] = useState('')
  const [maxGiB, setMaxGiB] = useState('')

  useEffect(() => {
    if (!open || !vmId) return
    setLoading(true)
    void queryVmLibvirt<CpuMemoryTopology>(vmId, 'cpu.memory.topology')
      .then((t) => {
        setCurrentGiB(String(Math.max(1, Math.round(t.current_memory_kib / 1024 / 1024))))
        setMaxGiB(String(Math.max(1, Math.round(t.max_memory_kib / 1024 / 1024))))
      })
      .catch((e: unknown) => onError(formatUserError(e)))
      .finally(() => setLoading(false))
  }, [open, vmId, onError])

  const save = async () => {
    setSaving(true)
    try {
      const maxMb = Number(maxGiB) * 1024
      const curMb = Number(currentGiB) * 1024
      if (maxMb > 0) {
        await setVmMemory(vmId, maxMb)
      }
      if (running && curMb > 0 && curMb <= maxMb) {
        await invokeVmLibvirt(vmId, 'live.memory', { memory_mb: curMb })
      }
      onNotify(`Memory updated for ${vmName}`)
      onSaved()
      onClose()
    } catch (e: unknown) {
      onError(formatUserError(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <MacSheet open={open} onClose={onClose} title={`Memory — ${vmName}`} ariaLabel="Memory sizing">
      {loading ? (
        <p className="text-sm text-[var(--text-muted)]">Loading memory…</p>
      ) : (
        <div className="space-y-4">
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Current memory (GiB)</span>
            <input type="number" min={1} className="input mt-1 w-full" value={currentGiB} onChange={(e) => setCurrentGiB(e.target.value)} />
            {running && <span className="text-xs text-[var(--text-muted)] mt-1 block">Balloon on running guest (requires guest balloon driver).</span>}
          </label>
          <label className="block text-sm">
            <span className="text-[var(--text-muted)]">Maximum memory (GiB)</span>
            <input type="number" min={1} className="input mt-1 w-full" value={maxGiB} onChange={(e) => setMaxGiB(e.target.value)} />
            <span className="text-xs text-[var(--text-muted)] mt-1 block">Persistent domain limit — may require reboot if lowered below current.</span>
          </label>
          <button type="button" className="btn-primary text-sm w-full" disabled={saving} onClick={() => void save()}>
            {saving ? 'Saving…' : 'Apply memory'}
          </button>
        </div>
      )}
    </MacSheet>
  )
}
