// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import * as chaos from '../api/chaos'
import { listVms, type NativeVm } from '../api/nativeVms'
import { listPlatformHosts, type PlatformHost } from '../api/platform'
import PageLayout from '../components/PageLayout'
import FleetCloudSubNav from '../components/FleetCloudSubNav'
import FleetCloudFooter from '../components/FleetCloudFooter'
import ConfirmDialog from '../components/ConfirmDialog'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'

const primary = 'btn btn-primary min-h-11'
const secondary = 'btn btn-secondary min-h-11'
const input = 'input min-h-11 w-full'
const muted = 'text-[var(--text-secondary)]'

type Kind = chaos.Step['kind']
const KINDS: { kind: Kind; label: string }[] = [
  { kind: 'latency', label: 'Network latency' },
  { kind: 'loss', label: 'Packet loss' },
  { kind: 'partition', label: 'Network partition' },
  { kind: 'disk', label: 'Disk throttle' },
  { kind: 'kill', label: 'Kill VMs' },
  { kind: 'host_failure', label: 'Host failure (simulated)' },
]

function newStep(kind: Kind, hosts: PlatformHost[]): chaos.Step {
  switch (kind) {
    case 'latency': return { kind, delay_ms: 200, jitter_ms: 20, secs: 60 }
    case 'loss': return { kind, loss_pct: 10, secs: 60 }
    case 'partition': return { kind, cidrs: [], peers: [], secs: 60 }
    case 'disk': return { kind, read_iops: 50, write_iops: 50, secs: 60 }
    case 'kill': return { kind, recover_secs: 180 }
    case 'host_failure': return { kind, host_id: hosts[0]?.id ?? '' }
  }
}

const blank = (): chaos.ExperimentBody => ({
  name: '',
  description: '',
  spec: { targets: [], steps: [], probes: [], abort: { min_success_pct: 50, window_secs: 20 }, baseline_secs: 15, recovery_secs: 15 },
})

const STATUS: Record<chaos.Run['status'], string> = {
  running: 'Running', passed: 'Passed', failed: 'Failed', aborted: 'Aborted', interrupted: 'Interrupted',
}

function Num({ label, value, onChange, min = 0, max }: { label: string; value: number; onChange: (n: number) => void; min?: number; max?: number }) {
  return (
    <label className="text-sm">
      {label}
      <input className={input} type="number" min={min} max={max} value={value} aria-label={label} onChange={(e) => onChange(Number(e.target.value))} />
    </label>
  )
}

function StepFields({ step, set, vms, hosts }: { step: chaos.Step; set: (s: chaos.Step) => void; vms: NativeVm[]; hosts: PlatformHost[] }) {
  switch (step.kind) {
    case 'latency':
      return <>
        <Num label="Delay ms" value={step.delay_ms} max={10000} onChange={(n) => set({ ...step, delay_ms: n })} />
        <Num label="Jitter ms" value={step.jitter_ms} max={10000} onChange={(n) => set({ ...step, jitter_ms: n })} />
        <Num label="Seconds" value={step.secs} min={1} max={3600} onChange={(n) => set({ ...step, secs: n })} />
      </>
    case 'loss':
      return <>
        <Num label="Loss percent" value={step.loss_pct} max={100} onChange={(n) => set({ ...step, loss_pct: n })} />
        <Num label="Seconds" value={step.secs} min={1} max={3600} onChange={(n) => set({ ...step, secs: n })} />
      </>
    case 'partition':
      return <>
        <label className="text-sm sm:col-span-2">
          Blocked networks (CIDRs, comma separated)
          <input className={input} aria-label="Blocked networks" value={step.cidrs.join(', ')}
            onChange={(e) => set({ ...step, cidrs: e.target.value.split(',').map((c) => c.trim()).filter(Boolean) })} />
        </label>
        <label className="text-sm">
          Peer VMs
          <select multiple className={input} aria-label="Peer VMs" value={step.peers}
            onChange={(e) => set({ ...step, peers: Array.from(e.target.selectedOptions, (o) => o.value) })}>
            {vms.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
          </select>
        </label>
        <Num label="Seconds" value={step.secs} min={1} max={3600} onChange={(n) => set({ ...step, secs: n })} />
      </>
    case 'disk':
      return <>
        <Num label="Read IOPS" value={step.read_iops} min={1} onChange={(n) => set({ ...step, read_iops: n })} />
        <Num label="Write IOPS" value={step.write_iops} min={1} onChange={(n) => set({ ...step, write_iops: n })} />
        <Num label="Seconds" value={step.secs} min={1} max={3600} onChange={(n) => set({ ...step, secs: n })} />
      </>
    case 'kill':
      return <Num label="Recovery wait seconds" value={step.recover_secs} min={10} max={3600} onChange={(n) => set({ ...step, recover_secs: n })} />
    case 'host_failure':
      return (
        <label className="text-sm">
          Host
          <select className={input} aria-label="Host" value={step.host_id} onChange={(e) => set({ ...step, host_id: e.target.value })}>
            {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
          </select>
        </label>
      )
  }
}

function ProbeFields({ probe, set, vms }: { probe: chaos.Probe; set: (p: chaos.Probe) => void; vms: NativeVm[] }) {
  switch (probe.kind) {
    case 'tcp':
      return <label className="text-sm sm:col-span-2">Address (host:port)<input className={input} aria-label="Probe address" value={probe.target} onChange={(e) => set({ ...probe, target: e.target.value })} /></label>
    case 'http':
      return <>
        <label className="text-sm sm:col-span-2">URL<input className={input} aria-label="Probe URL" value={probe.url} onChange={(e) => set({ ...probe, url: e.target.value })} /></label>
        <Num label="Expected status" value={probe.expect_status} min={100} max={599} onChange={(n) => set({ ...probe, expect_status: n })} />
      </>
    case 'vm_running':
      return (
        <label className="text-sm sm:col-span-2">
          VM
          <select className={input} aria-label="Probe VM" value={probe.vm} onChange={(e) => set({ ...probe, vm: e.target.value })}>
            {vms.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
          </select>
        </label>
      )
  }
}

function Editor({ initial, vms, hosts, onSave, onCancel }: {
  initial: chaos.ExperimentBody
  vms: NativeVm[]
  hosts: PlatformHost[]
  onSave: (b: chaos.ExperimentBody) => Promise<void>
  onCancel: () => void
}) {
  const [b, setB] = useState(initial)
  const [busy, setBusy] = useState(false)
  const spec = b.spec
  const setSpec = (s: Partial<chaos.ExperimentSpec>) => setB({ ...b, spec: { ...spec, ...s } })
  const [addKind, setAddKind] = useState<Kind>('latency')
  const [probeKind, setProbeKind] = useState<chaos.Probe['kind']>('tcp')

  const addProbe = () => {
    const p: chaos.Probe = probeKind === 'tcp' ? { kind: 'tcp', target: '' }
      : probeKind === 'http' ? { kind: 'http', url: '', expect_status: 200 }
      : { kind: 'vm_running', vm: spec.targets[0] ?? vms[0]?.id ?? '' }
    setSpec({ probes: [...spec.probes, p] })
  }

  return (
    <section className="tahoe-glass-card space-y-4 p-5" aria-label="Experiment editor">
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="text-sm">Name<input className={input} aria-label="Experiment name" value={b.name} onChange={(e) => setB({ ...b, name: e.target.value })} /></label>
        <label className="text-sm">Description<input className={input} aria-label="Description" value={b.description} onChange={(e) => setB({ ...b, description: e.target.value })} /></label>
      </div>

      <fieldset className="space-y-2">
        <legend className="font-semibold">Targets</legend>
        <p className={`text-sm ${muted}`}>Faults hit these VMs. VMs tagged <code>chaos=protected</code> can't be chosen.</p>
        <div className="flex flex-wrap gap-3">
          {vms.map((v) => (
            <label key={v.id} className="flex min-h-11 items-center gap-2 text-sm">
              <input type="checkbox" checked={spec.targets.includes(v.id)}
                onChange={(e) => setSpec({ targets: e.target.checked ? [...spec.targets, v.id] : spec.targets.filter((t) => t !== v.id) })} />
              {v.name}
            </label>
          ))}
        </div>
      </fieldset>

      <fieldset className="space-y-2">
        <legend className="font-semibold">Steps, in order</legend>
        <ol className="space-y-3">
          {spec.steps.map((s, i) => (
            <li key={i} className="space-y-2 rounded-xl border border-[var(--apple-hairline)] p-3" aria-label={`Step ${i + 1}`}>
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium">{i + 1}. {KINDS.find((k) => k.kind === s.kind)?.label}</span>
                <button type="button" className={secondary} onClick={() => setSpec({ steps: spec.steps.filter((_, j) => j !== i) })} aria-label={`Remove step ${i + 1}`}>Remove</button>
              </div>
              <div className="grid gap-3 sm:grid-cols-3">
                <StepFields step={s} vms={vms} hosts={hosts} set={(n) => setSpec({ steps: spec.steps.map((x, j) => (j === i ? n : x)) })} />
              </div>
            </li>
          ))}
        </ol>
        <div className="flex flex-wrap items-end gap-3">
          <label className="text-sm">
            Step kind
            <select className={input} aria-label="Step kind" value={addKind} onChange={(e) => setAddKind(e.target.value as Kind)}>
              {KINDS.map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
            </select>
          </label>
          <button type="button" className={secondary} onClick={() => setSpec({ steps: [...spec.steps, newStep(addKind, hosts)] })}>Add step</button>
        </div>
      </fieldset>

      <fieldset className="space-y-2">
        <legend className="font-semibold">Health probes</legend>
        <p className={`text-sm ${muted}`}>Checked every 2 seconds from the controller. The run stops and lifts every fault if they fail too often.</p>
        <ul className="space-y-3">
          {spec.probes.map((p, i) => (
            <li key={i} className="grid items-end gap-3 sm:grid-cols-4" aria-label={`Probe ${i + 1}`}>
              <ProbeFields probe={p} vms={vms} set={(n) => setSpec({ probes: spec.probes.map((x, j) => (j === i ? n : x)) })} />
              <button type="button" className={secondary} onClick={() => setSpec({ probes: spec.probes.filter((_, j) => j !== i) })} aria-label={`Remove probe ${i + 1}`}>Remove</button>
            </li>
          ))}
        </ul>
        <div className="flex flex-wrap items-end gap-3">
          <label className="text-sm">
            Probe kind
            <select className={input} aria-label="Probe kind" value={probeKind} onChange={(e) => setProbeKind(e.target.value as chaos.Probe['kind'])}>
              <option value="tcp">TCP connect</option>
              <option value="http">HTTP status</option>
              <option value="vm_running">VM running</option>
            </select>
          </label>
          <button type="button" className={secondary} onClick={addProbe}>Add probe</button>
        </div>
        <div className="grid gap-3 sm:grid-cols-4">
          <Num label="Abort below success percent" value={spec.abort.min_success_pct} max={100} onChange={(n) => setSpec({ abort: { ...spec.abort, min_success_pct: n } })} />
          <Num label="Over the last seconds" value={spec.abort.window_secs} min={4} max={600} onChange={(n) => setSpec({ abort: { ...spec.abort, window_secs: n } })} />
          <Num label="Baseline seconds" value={spec.baseline_secs} max={600} onChange={(n) => setSpec({ baseline_secs: n })} />
          <Num label="Recovery seconds" value={spec.recovery_secs} max={600} onChange={(n) => setSpec({ recovery_secs: n })} />
        </div>
      </fieldset>

      <div className="flex flex-wrap gap-3">
        <button type="button" className={primary} disabled={busy} onClick={async () => { setBusy(true); try { await onSave(b) } finally { setBusy(false) } }}>
          {busy ? 'Saving…' : 'Save experiment'}
        </button>
        <button type="button" className={secondary} onClick={onCancel}>Cancel</button>
      </div>
    </section>
  )
}

function pct(n: number | null | undefined) {
  return n == null ? '—' : `${Math.round(n)}%`
}

function PhaseTable({ phases, current }: { phases: chaos.PhaseReport[]; current?: number }) {
  return (
    <table className="w-full text-sm" aria-label="Phases">
      <thead><tr className={`text-left ${muted}`}><th className="py-1">Phase</th><th>Probe success</th><th>p50 / p95</th><th>Notes</th></tr></thead>
      <tbody>
        {phases.map((p, i) => (
          <tr key={i} className="border-t border-[var(--apple-hairline)] align-top" aria-current={current === i ? 'step' : undefined}>
            <td className="py-2 pr-3">{p.name}{current === i && <span className={muted}> · now</span>}</td>
            <td className="pr-3">{pct(p.success_pct)} <span className={muted}>({p.ok}/{p.samples})</span></td>
            <td className="pr-3">{p.p50_ms ?? '—'} / {p.p95_ms ?? '—'} ms</td>
            <td>
              {[...(p.notes ?? []), ...(p.recovered_s != null ? [`back after ${p.recovered_s.toFixed(0)} s`] : [])].join('; ')}
              {p.error && <span className="text-[var(--apple-red,#d70015)]"> {p.error}</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function RunView({ runId, onDone }: { runId: string; onDone: () => void }) {
  const toast = useToastContext()
  const [data, setData] = useState<{ run: chaos.Run; live: chaos.LiveRun | null } | null>(null)

  useEffect(() => {
    let stop = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const tick = async () => {
      try {
        const d = await chaos.getRun(runId)
        if (stop) return
        setData(d)
        if (d.run.status === 'running') timer = setTimeout(tick, 2000)
        else onDone()
      } catch (e) {
        if (!stop) toast.error(formatUserError(e))
      }
    }
    void tick()
    return () => { stop = true; clearTimeout(timer) }
  }, [runId, onDone, toast])

  if (!data) return <p role="status">Loading run…</p>
  const { run, live } = data
  const report = run.report && 'phases' in run.report ? (run.report as chaos.Report) : null
  return (
    <section className="tahoe-glass-card space-y-3 p-5" aria-label="Run">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-semibold">Run: {STATUS[run.status]}</h2>
        {run.status === 'running' && (
          <button className={secondary} onClick={() => chaos.abortRun(run.id).then(() => toast.success('Stopping. Every fault is being lifted.')).catch((e) => toast.error(formatUserError(e)))}>
            Abort and lift faults
          </button>
        )}
      </div>
      {run.abort_reason && <p>{run.abort_reason}</p>}
      {live && <PhaseTable phases={live.phases} current={live.phase} />}
      {!live && report && (
        <>
          <p className={muted}>Took {report.duration_s.toFixed(0)} s</p>
          <PhaseTable phases={report.phases} />
          {report.findings.length > 0 && (
            <div>
              <h3 className="font-semibold">Findings</h3>
              <ul className="list-disc pl-5">{report.findings.map((f, i) => <li key={i}>{f}</li>)}</ul>
            </div>
          )}
        </>
      )}
    </section>
  )
}

export default function FleetCloudChaos() {
  const toast = useToastContext()
  const [items, setItems] = useState<chaos.Experiment[] | null>(null)
  const [faults, setFaults] = useState<chaos.ActiveFault[]>([])
  const [vms, setVms] = useState<NativeVm[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [error, setError] = useState<string | null>(null)
  const [editing, setEditing] = useState<{ id: string | null; body: chaos.ExperimentBody } | null>(null)
  const [confirm, setConfirm] = useState<chaos.Experiment | null>(null)
  const [removing, setRemoving] = useState<chaos.Experiment | null>(null)
  const [runId, setRunId] = useState<string | null>(null)

  const load = useCallback(async () => {
    const [e, f] = await Promise.all([chaos.listExperiments(), chaos.listFaults().catch(() => ({ items: [] }))])
    setItems(e); setFaults(f.items); setError(null)
  }, [])

  useEffect(() => {
    load().catch((e) => setError(formatUserError(e)))
    listVms().then(setVms).catch(() => setVms([]))
    listPlatformHosts().then(setHosts).catch(() => setHosts([]))
  }, [load])

  const refresh = useCallback(() => { load().catch(() => undefined) }, [load])
  const vmName = (id: string) => vms.find((v) => v.id === id)?.name ?? id.slice(0, 8)

  async function save(b: chaos.ExperimentBody) {
    try {
      if (editing?.id) await chaos.updateExperiment(editing.id, b)
      else await chaos.createExperiment(b)
      toast.success(`Saved ${b.name}.`)
      setEditing(null)
      await load()
    } catch (e) {
      toast.error(formatUserError(e))
    }
  }

  async function run(e: chaos.Experiment) {
    setConfirm(null)
    try {
      const r = await chaos.runExperiment(e.id, e.name)
      setRunId(r.run_id)
      await load()
    } catch (err) {
      toast.error(formatUserError(err))
    }
  }

  async function remove(e: chaos.Experiment) {
    setRemoving(null)
    try {
      await chaos.deleteExperiment(e.id)
      toast.success(`Deleted ${e.name}.`)
      await load()
    } catch (err) {
      toast.error(formatUserError(err))
    }
  }

  async function openLast(e: chaos.Experiment) {
    try {
      const d = await chaos.getExperiment(e.id)
      if (d.runs[0]) setRunId(d.runs[0].id)
    } catch (err) {
      toast.error(formatUserError(err))
    }
  }

  return (
    <PageLayout
      title="Game days"
      subtitle="Break things on purpose: inject latency, loss, partitions, slow disks and crashes into your own VMs, watch health probes, and get a report."
      prepend={<FleetCloudSubNav />}
      error={error}
    >
      <div className="space-y-6">
        {faults.length > 0 && (
          <section className="tahoe-glass-card space-y-2 p-5" aria-label="Active faults">
            <h2 className="text-lg font-semibold">Faults active now</h2>
            <ul className="text-sm">
              {faults.map((f) => (
                <li key={f.id}>
                  {f.vm} on {f.host}: {[f.delay_ms ? `${f.delay_ms} ms delay` : '', f.loss_pct ? `${f.loss_pct}% loss` : '', f.partition.length ? `blocked from ${f.partition.join(', ')}` : ''].filter(Boolean).join(', ')} · ends in {f.remaining_secs} s
                  {f.error && <span className="text-[var(--apple-red,#d70015)]"> · {f.error}</span>}
                </li>
              ))}
            </ul>
          </section>
        )}

        {runId && <RunView key={runId} runId={runId} onDone={refresh} />}

        {editing ? (
          <Editor initial={editing.body} vms={vms} hosts={hosts} onSave={save} onCancel={() => setEditing(null)} />
        ) : (
          <section className="tahoe-glass-card space-y-3 p-5" aria-label="Experiments">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <h2 className="text-lg font-semibold">Experiments</h2>
              <button className={primary} onClick={() => setEditing({ id: null, body: blank() })}>New experiment</button>
            </div>
            <p className={muted}>
              Every network fault runs under a lease on the host and lifts itself if the controller goes away. Host failure is a
              simulation of what would be affected; nothing is shut down.
            </p>
            {!items && !error && <p role="status">Loading…</p>}
            {items && items.length === 0 && <p>No experiments yet. Start with latency on one VM and a probe on its service.</p>}
            <ul className="divide-y divide-[var(--apple-hairline)]">
              {items?.map((e) => (
                <li key={e.id} className="flex flex-wrap items-center gap-3 py-3">
                  <div className="min-w-0 flex-1">
                    <p className="font-medium">
                      {e.name}
                      {e.last_status && <span className={muted}> · last run {STATUS[e.last_status as chaos.Run['status']] ?? e.last_status}</span>}
                    </p>
                    <p className="text-sm">{e.spec.steps.map(chaos.stepLabel).join(' → ') || 'No steps'}</p>
                    <p className={`text-xs ${muted}`}>
                      {e.spec.targets.map(vmName).join(', ')} · {e.spec.probes.length} {e.spec.probes.length === 1 ? 'probe' : 'probes'}
                    </p>
                  </div>
                  {e.last_status && <button className={secondary} onClick={() => void openLast(e)} aria-label={`Show last run of ${e.name}`}>Last run</button>}
                  <button className={secondary} disabled={e.last_status === 'running'} onClick={() => setEditing({ id: e.id, body: { name: e.name, description: e.description, spec: e.spec } })} aria-label={`Edit ${e.name}`}>Edit</button>
                  <button className={secondary} disabled={e.last_status === 'running'} onClick={() => setRemoving(e)} aria-label={`Delete ${e.name}`}>Delete</button>
                  <button className={primary} disabled={e.last_status === 'running'} onClick={() => setConfirm(e)} aria-label={`Run ${e.name}`}>Run</button>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
      <ConfirmDialog
        open={confirm !== null}
        title={`Run ${confirm?.name ?? ''}?`}
        message={confirm ? `${confirm.spec.steps.map(chaos.stepLabel).join(', then ')}. Targets: ${confirm.spec.targets.map(vmName).join(', ')}.` : ''}
        confirmLabel="Start game day"
        variant="warning"
        typeToMatch={confirm?.name}
        onConfirm={() => confirm && void run(confirm)}
        onCancel={() => setConfirm(null)}
      />
      <ConfirmDialog
        open={removing !== null}
        title={`Delete ${removing?.name ?? ''}?`}
        message="Its run history goes with it."
        confirmLabel="Delete"
        onConfirm={() => removing && void remove(removing)}
        onCancel={() => setRemoving(null)}
      />
      <FleetCloudFooter />
    </PageLayout>
  )
}
