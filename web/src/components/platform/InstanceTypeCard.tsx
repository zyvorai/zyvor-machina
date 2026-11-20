// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Cpu } from 'lucide-react'
import { listFlavors, type NativeFlavor } from '../../api/flavors'
import { changeVmType } from '../../api/nativeVms'
import { formatUserError } from '../../utils/apiError'

type Props = {
  vm: { id: string; name: string; vcpus: number; memory_mib: number; flavor_id?: string | null; observed_state: string }
  /** Called after the change has been queued so the page can refresh. */
  onQueued?: () => void
}

const sizeText = (vcpus: number, memoryMib: number) =>
  `${vcpus} vCPU · ${memoryMib >= 1024 ? `${+(memoryMib / 1024).toFixed(1)} GiB` : `${memoryMib} MiB`}`

/**
 * EC2's "instance type", on one machine: what it is now, and a guarded way to change it. A running machine is shut down
 * cleanly and started again, which the card says before anything happens.
 */
export default function InstanceTypeCard({ vm, onQueued }: Props) {
  const [flavors, setFlavors] = useState<NativeFlavor[] | null>(null)
  const [choice, setChoice] = useState('')
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [queued, setQueued] = useState(false)

  useEffect(() => {
    let alive = true
    listFlavors().then((f) => { if (alive) setFlavors(Array.isArray(f) ? f : []) }).catch(() => { if (alive) setFlavors([]) })
    return () => { alive = false }
  }, [])

  const current = flavors?.find((f) => f.id === vm.flavor_id)
  const options = (flavors ?? []).filter((f) => f.id !== vm.flavor_id || f.vcpus !== vm.vcpus || f.memory_mib !== vm.memory_mib)
  const picked = options.find((f) => f.id === choice)
  const running = /running/i.test(vm.observed_state)

  const submit = async () => {
    if (!picked) return
    setBusy(true); setError(null)
    try {
      await changeVmType(vm.id, picked.id)
      setQueued(true); setConfirming(false); setChoice('')
      onQueued?.()
    } catch (e) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4" aria-label="Instance type" data-testid="instance-type">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-medium text-[var(--text-secondary)]"><Cpu className="h-4 w-4" aria-hidden /> Instance type</h2>
      <p className="text-sm text-[var(--text-primary)]">
        <span className="font-medium">{current ? current.name : 'Custom size'}</span>
        <span className="text-[var(--text-secondary)]"> · {sizeText(vm.vcpus, vm.memory_mib)}</span>
      </p>
      {flavors && options.length > 0 ? (
        <div className="mt-3 flex flex-wrap items-end gap-3">
          <label className="flex w-full flex-col gap-1 sm:w-auto">
            <span className="text-xs text-[var(--text-secondary)]">Change to</span>
            <select aria-label="New instance type" className="form-selector min-h-11 w-full p-2 sm:w-auto" value={choice} onChange={(e) => { setChoice(e.target.value); setConfirming(false); setQueued(false) }}>
              <option value="">Choose a type…</option>
              {options.map((f) => <option key={f.id} value={f.id}>{f.name} — {sizeText(f.vcpus, f.memory_mib)}</option>)}
            </select>
          </label>
          {!confirming ? (
            <button type="button" className="btn btn-secondary min-h-11" disabled={!picked || busy} onClick={() => setConfirming(true)}>Change type…</button>
          ) : null}
        </div>
      ) : null}
      {confirming && picked ? (
        <div className="mt-3 rounded-xl bg-[var(--apple-fill-tertiary)]/60 p-3 text-sm" role="group" aria-label="Confirm instance type change">
          <p>
            {running
              ? `${vm.name} will be shut down cleanly, resized to ${picked.name} (${sizeText(picked.vcpus, picked.memory_mib)}) and started again. It is unavailable for a short while.`
              : `${vm.name} is stopped; it will be resized to ${picked.name} (${sizeText(picked.vcpus, picked.memory_mib)}). It stays stopped.`}
            {' '}Its disks are not changed.
          </p>
          <div className="mt-2 flex gap-2">
            <button type="button" className="btn btn-primary min-h-11" disabled={busy} onClick={() => void submit()}>{running ? 'Shut down and resize' : 'Resize'}</button>
            <button type="button" className="btn btn-secondary min-h-11" disabled={busy} onClick={() => setConfirming(false)}>Cancel</button>
          </div>
        </div>
      ) : null}
      {queued ? <p className="mt-2 text-xs text-emerald-600" role="status">Change queued. The type updates when the task finishes.</p> : null}
      {error ? <p className="mt-2 text-xs text-red-500" role="alert">{error}</p> : null}
    </section>
  )
}
