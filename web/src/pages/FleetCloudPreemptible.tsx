// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import * as preempt from '../api/preempt'
import { listVms, type NativeVm } from '../api/nativeVms'
import PageLayout from '../components/PageLayout'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'

const primary = 'btn btn-primary min-h-11'
const secondary = 'btn btn-secondary min-h-11'
const input = 'input min-h-11'
const muted = 'text-[var(--text-secondary)]'

function gib(mib: number) {
  return `${(mib / 1024).toFixed(mib >= 10240 ? 0 : 1)} GiB`
}

function stateOf(v: preempt.PreemptibleVm) {
  if (v.preempted_at) return `Preempted since ${v.preempted_at.replace('T', ' ').slice(0, 16)}`
  if (v.desired_state === 'sleeping') return 'Sleeping (idle)'
  return v.observed_state === 'running' ? 'Running' : v.observed_state
}

export default function FleetCloudPreemptible() {
  const toast = useToastContext()
  const [data, setData] = useState<preempt.PreemptOverview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [reserve, setReserve] = useState('10')
  const [enabled, setEnabled] = useState(true)
  const [vms, setVms] = useState<NativeVm[]>([])
  const [addId, setAddId] = useState('')
  const [addPriority, setAddPriority] = useState('0')
  const [priorities, setPriorities] = useState<Record<string, string>>({})

  const load = useCallback(async () => {
    const d = await preempt.getPreemption()
    setData(d)
    setReserve(String(d.settings.reserve_pct))
    setEnabled(d.settings.enabled)
    setError(null)
  }, [])

  useEffect(() => {
    load().catch((e) => setError(formatUserError(e)))
    listVms().then(setVms).catch(() => setVms([]))
  }, [load])

  async function act(work: () => Promise<string>) {
    setBusy(true)
    try {
      toast.success(await work())
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const reserveNum = Number(reserve)
  const reserveOk = Number.isInteger(reserveNum) && reserveNum >= 0 && reserveNum <= 90
  const flagged = new Set(data?.vms.map((v) => v.id))
  const addable = vms.filter((v) => !flagged.has(v.id))

  return (
    <PageLayout
      title="Preemptible instances"
      subtitle="Instances that give way when a host runs short of memory: saved to disk instead of stopped, and back by themselves when there is room."
      prepend={<FleetCloudSubNav />}
      error={error}
    >
      <div className="space-y-6">
        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Settings">
          <h2 className="text-lg font-semibold">Capacity reserve</h2>
          <p className={muted}>
            Each host keeps this much memory free. Below it, preemptible instances on that host are saved to disk, lowest priority
            first. They come back, highest priority first, once the host has the reserve plus {data?.resume_margin_pct ?? 5}% to spare.
            A regular instance that fits nowhere also makes room this way.
          </p>
          <div className="flex flex-wrap items-end gap-3">
            <label className="flex min-h-11 items-center gap-2 text-sm">
              <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
              Preemption on
            </label>
            <label className="text-sm">
              Free memory reserve (%)
              <input className={`${input} w-28`} aria-label="Free memory reserve" inputMode="numeric" value={reserve}
                aria-invalid={!reserveOk} onChange={(e) => setReserve(e.target.value)} />
            </label>
            <button className={primary} disabled={busy || !reserveOk}
              onClick={() => void act(async () => {
                await preempt.updatePreemptSettings({ enabled, reserve_pct: reserveNum })
                return 'Saved.'
              })}>
              Save
            </button>
          </div>
        </section>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Hosts">
          <h2 className="text-lg font-semibold">Hosts</h2>
          {!data && !error && <p role="status">Loading…</p>}
          <ul className="divide-y divide-[var(--apple-hairline)]">
            {data?.hosts.map((h) => {
              const short = h.free_mib < h.reserve_mib
              return (
                <li key={h.id} className="py-3">
                  <p className="font-medium">
                    {h.name}
                    {short && <span className="text-[var(--apple-red,#d70015)]"> · below its reserve</span>}
                    {!h.fresh && <span className={muted}> · no recent heartbeat, left alone</span>}
                  </p>
                  <p className="text-sm">
                    {gib(h.free_mib)} free of {gib(h.total_mib)} · reserve {gib(h.reserve_mib)} · {gib(h.preemptible_running_mib)} in running preemptible instances
                  </p>
                </li>
              )
            })}
          </ul>
        </section>

        <section className="tahoe-glass-card space-y-3 p-5" aria-label="Preemptible instances">
          <h2 className="text-lg font-semibold">Instances</h2>
          {data && data.vms.length === 0 && <p>No preemptible instances yet. Mark one below, or tick Preemptible when you create it.</p>}
          <ul className="divide-y divide-[var(--apple-hairline)]">
            {data?.vms.map((v) => (
              <li key={v.id} className="flex flex-wrap items-center gap-3 py-3">
                <div className="min-w-0 flex-1">
                  <p className="font-medium">{v.name}{v.project ? <span className={muted}> · {v.project}</span> : null}</p>
                  <p className="text-sm">{stateOf(v)} · {gib(v.memory_mib)} on {v.host ?? 'no host'}</p>
                </div>
                <label className="text-sm">
                  Priority
                  <input className={`${input} w-24`} aria-label={`Priority of ${v.name}`} inputMode="numeric" value={priorities[v.id] ?? String(v.priority)}
                    onChange={(e) => setPriorities({ ...priorities, [v.id]: e.target.value })} />
                </label>
                <button className={secondary} disabled={busy}
                  onClick={() => void act(async () => {
                    await preempt.setVmPreemptible(v.id, true, Number(priorities[v.id] ?? v.priority))
                    setPriorities(({ [v.id]: _, ...rest }) => rest)
                    return `Priority of ${v.name} saved.`
                  })}
                  aria-label={`Save priority of ${v.name}`}>
                  Save
                </button>
                <button className={secondary} disabled={busy}
                  onClick={() => void act(async () => {
                    await preempt.setVmPreemptible(v.id, false, 0)
                    return v.preempted_at ? `${v.name} is no longer preemptible; it now wakes on traffic like any sleeping instance.` : `${v.name} is no longer preemptible.`
                  })}
                  aria-label={`Stop preempting ${v.name}`}>
                  Make regular
                </button>
              </li>
            ))}
          </ul>
          <div className="flex flex-wrap items-end gap-3">
            <label className="text-sm">
              Instance
              <select className={input} aria-label="Instance to make preemptible" value={addId} onChange={(e) => setAddId(e.target.value)}>
                <option value="">Choose…</option>
                {addable.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
              </select>
            </label>
            <label className="text-sm">
              Priority
              <input className={`${input} w-24`} aria-label="New priority" inputMode="numeric" value={addPriority} onChange={(e) => setAddPriority(e.target.value)} />
            </label>
            <button className={primary} disabled={busy || !addId}
              onClick={() => void act(async () => {
                await preempt.setVmPreemptible(addId, true, Number(addPriority))
                const name = vms.find((v) => v.id === addId)?.name ?? 'Instance'
                setAddId('')
                return `${name} is now preemptible.`
              })}>
              Make preemptible
            </button>
          </div>
        </section>

        {data && data.events.length > 0 && (
          <section className="tahoe-glass-card space-y-2 p-5" aria-label="Recent preemptions">
            <h2 className="text-lg font-semibold">Recent</h2>
            <ul className="text-sm">
              {data.events.map((e, i) => (
                <li key={i}>
                  <span className={muted}>{e.at.replace('T', ' ').slice(0, 16)}</span> {e.vm}: {e.kind === 'sleep' ? e.reason : `resumed, ${e.reason}`}
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
      <FleetCloudFooter />
    </PageLayout>
  )
}
