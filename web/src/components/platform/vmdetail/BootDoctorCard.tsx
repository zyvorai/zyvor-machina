// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { AlertTriangle, Check, Loader2, Stethoscope } from 'lucide-react'
import { getPlatformVm, runVmHealthCheck, vmPower, type PlatformVm } from '../../../api/platform'
import {
  guestkitCapabilities,
  diagnoseGuestDisk,
  extractBackupPath,
  repairGuestDisk,
  summariseDoctorOutput,
  type GuestRepairReport,
  type GuestkitCapabilities,
} from '../../../api/guestRepair'
import { formatUserError } from '../../../utils/apiError'
import { vmHealthScore } from '../../../utils/vmHealthScore'

type Phase = 'idle' | 'working' | 'diagnosed' | 'previewed' | 'repaired' | 'failed'

const OFF = (s: string) => /^(shutoff|shut off|stopped|off)$/i.test(s.trim())
const ON = (s: string) => /^running$/i.test(s.trim())
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
async function waitFor(check: () => Promise<boolean>, timeoutMs: number, everyMs = 3000): Promise<boolean> {
  const end = Date.now() + timeoutMs
  while (Date.now() < end) {
    if (await check().catch(() => false)) return true
    await sleep(everyMs)
  }
  return false
}

/**
 * Boot Doctor: look inside a machine's disk (powered off), see what GuestKit would change, repair with a backup,
 * start it and check it comes up. Nothing is written until you press Repair.
 */
export default function BootDoctorCard({ vm, onChanged, onEngaged }: { vm: PlatformVm; onChanged?: () => void; onEngaged?: () => void }) {
  const [caps, setCaps] = useState<GuestkitCapabilities | null | undefined>(undefined)
  const [phase, setPhase] = useState<Phase>('idle')
  const [status, setStatus] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [diagnosis, setDiagnosis] = useState<GuestRepairReport | null>(null)
  const [preview, setPreview] = useState<GuestRepairReport | null>(null)
  const [result, setResult] = useState<{ report: GuestRepairReport; started: boolean; score: number | null } | null>(null)
  const [confirmed, setConfirmed] = useState(false)

  useEffect(() => {
    let alive = true
    void guestkitCapabilities().then((c) => { if (alive) setCaps(c) })
    return () => { alive = false }
  }, [])

  // Make sure the VM is off before touching its disk. Never force-stops.
  const ensureOff = async (): Promise<boolean> => {
    const cur = await getPlatformVm(vm.id)
    if (OFF(cur.observed_state)) return false
    setStatus('Shutting the machine down cleanly…')
    await vmPower(vm.id, 'shutdown')
    const off = await waitFor(async () => OFF((await getPlatformVm(vm.id)).observed_state), 150_000)
    if (!off) throw new Error('The guest did not shut down within 2.5 minutes. Nothing was changed — shut it down yourself and try again.')
    return true
  }

  const guard = async (fn: () => Promise<void>) => {
    // Tell the page to keep this card on screen even after the VM is running again.
    onEngaged?.()
    setPhase('working')
    setError(null)
    try {
      await fn()
    } catch (e: unknown) {
      setError(formatUserError(e))
      setPhase('failed')
    } finally {
      setStatus(null)
    }
  }

  const diagnose = () => guard(async () => {
    await ensureOff()
    setStatus('Looking inside the disk with GuestKit…')
    setDiagnosis(await diagnoseGuestDisk(vm.name))
    setPhase('diagnosed')
  })

  const previewFix = () => guard(async () => {
    setStatus('Working out what would change…')
    setPreview(await repairGuestDisk(vm.name, { dryRun: true }))
    setPhase('previewed')
  })

  const repair = () => guard(async () => {
    setStatus('Backing the disk up and repairing it…')
    const report = await repairGuestDisk(vm.name, { dryRun: false, backup: true })
    setStatus('Starting the machine…')
    await vmPower(vm.id, 'start')
    const started = await waitFor(async () => ON((await getPlatformVm(vm.id)).observed_state), 90_000)
    let score: number | null = null
    if (started) {
      setStatus('Checking it came up healthy…')
      await sleep(8000)
      score = vmHealthScore(await runVmHealthCheck(vm.id).catch(() => null))
    }
    setResult({ report, started, score })
    setPhase('repaired')
    onChanged?.()
  })

  // Daemon too old to say (null) or still checking (undefined): stay out of the way.
  if (!caps) return null
  if (!caps.cli_found) {
    return (
      <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]/40 p-4 text-xs text-[var(--text-secondary)]" data-testid="boot-doctor-missing-guestkit">
        <p className="flex items-center gap-2 text-sm font-medium text-[var(--text-primary)]"><Stethoscope className="h-4 w-4" /> Boot Doctor</p>
        <p className="mt-1">Boot Doctor needs GuestKit on this hypervisor (the <code>guestkit</code> command was not found). Install GuestKit and this card turns on.</p>
      </section>
    )
  }

  const d = diagnosis ? summariseDoctorOutput(diagnosis.output) : null
  const p = preview ? summariseDoctorOutput(preview.output) : null
  const busy = phase === 'working'

  return (
    <section className="rounded-2xl border border-[color-mix(in_srgb,#bf5af2_32%,transparent)] bg-[color-mix(in_srgb,#bf5af2_6%,transparent)] p-4" data-testid="boot-doctor">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="flex items-center gap-2 font-medium text-[var(--text-primary)]"><Stethoscope className="h-4 w-4 text-[#bf5af2]" /> Boot Doctor</p>
          <p className="mt-1 text-xs text-[var(--text-secondary)]">
            Won't boot or acting strange? Machina powers it off, looks inside the disk with GuestKit and fixes boot problems (bootloader, fstab) — with a backup first.
            Nothing is written until you press Repair.
          </p>
        </div>
        {phase === 'idle' || phase === 'failed' ? (
          <button type="button" className="btn-secondary text-sm" onClick={() => void diagnose()}>
            {ON(vm.observed_state) ? 'Shut down & check disk' : 'Check disk'}
          </button>
        ) : null}
      </div>

      {busy ? (
        <p className="mt-3 flex items-center gap-2 text-sm text-[var(--text-secondary)]"><Loader2 className="h-4 w-4 animate-spin" /> {status ?? 'Working…'}</p>
      ) : null}

      {d && (phase === 'diagnosed' || phase === 'previewed' || phase === 'repaired') ? (
        <div className="mt-3 space-y-2 rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-3">
          <p className="flex items-center gap-2 text-sm font-medium text-[var(--text-primary)]">
            Findings{d.score != null ? <span className="rounded-full bg-[var(--apple-fill-tertiary)] px-2 py-0.5 text-xs tabular-nums">boot score {d.score}/100</span> : null}
          </p>
          {d.findings.length > 0 ? (
            <ul className="list-disc space-y-1 pl-5 text-xs text-[var(--text-secondary)]">{d.findings.map((f) => <li key={f}>{f}</li>)}</ul>
          ) : (
            <pre className="max-h-48 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-[var(--text-muted)]">{d.raw}</pre>
          )}
        </div>
      ) : null}

      {phase === 'diagnosed' ? (
        <div className="mt-3 flex flex-wrap gap-2">
          <button type="button" className="btn-primary text-sm" onClick={() => void previewFix()}>Preview the fix</button>
          <button type="button" className="btn-secondary text-sm" onClick={() => { setPhase('idle'); setDiagnosis(null) }}>Close</button>
        </div>
      ) : null}

      {p && phase === 'previewed' ? (
        <div className="mt-3 space-y-3">
          <div className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-3">
            <p className="text-sm font-medium text-[var(--text-primary)]">What Repair would change (preview — nothing written yet)</p>
            <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-[var(--text-secondary)]">{p.raw || 'No changes needed.'}</pre>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
              Back up the disk, repair it and start {vm.name}.
            </label>
            <button type="button" className="btn-primary text-sm" disabled={!confirmed} onClick={() => void repair()}>Repair now</button>
          </div>
        </div>
      ) : null}

      {phase === 'repaired' && result ? (
        <div className="mt-3 space-y-2 text-sm" role="status">
          <p className="flex items-center gap-2 text-emerald-600"><Check className="h-4 w-4" /> Repair applied{result.report.backup ? ' (disk backed up first)' : ''}.</p>
          {extractBackupPath(result.report.output) ? (
            <p className="text-xs text-[var(--text-muted)]">Backup kept at <code className="break-all">{extractBackupPath(result.report.output)}</code> — restore it to undo the repair.</p>
          ) : null}
          <p className="text-xs text-[var(--text-secondary)]">
            {result.started
              ? result.score != null ? `The machine is running again — health ${result.score}/100.` : 'The machine is running again.'
              : 'The repair finished, but the machine did not report running within 90 seconds — check its console.'}
          </p>
        </div>
      ) : null}

      {error ? (
        <p className="mt-3 flex items-start gap-2 text-xs text-red-500" role="alert"><AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" /> {error}</p>
      ) : null}
    </section>
  )
}
