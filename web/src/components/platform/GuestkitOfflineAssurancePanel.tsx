// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { HardDrive, Loader2, Stethoscope, Route } from 'lucide-react'
import { Link } from 'react-router'
import {
  getGuestkitStatus,
  guestkitVmDoctor,
  guestkitVmMigratePlan,
  type GuestkitDoctorReport,
  type GuestkitMigratePlanReport,
} from '../../api/guestkit'
import { formatUserError } from '../../utils/apiError'
import { statusPillClasses, statusSurfaceClasses, statusToneClass } from '../../utils/semanticColors'
import { hubLinkClasses } from '../../utils/semanticColors'

type Props = {
  vmId: string
  vmState?: string
  guestkitEnabled: boolean
  /** When true (e.g. from Doctor tab deep link), run migrate plan once on mount. */
  autoRunMigratePlan?: boolean
}

function scoreTone(score: number): 'ok' | 'warn' | 'error' {
  if (score >= 75) return 'ok'
  if (score >= 50) return 'warn'
  return 'error'
}

export default function GuestkitOfflineAssurancePanel({ vmId, vmState, guestkitEnabled, autoRunMigratePlan = false }: Props) {
  const [gkReachable, setGkReachable] = useState<boolean | null>(null)
  const [doctor, setDoctor] = useState<GuestkitDoctorReport | null>(null)
  const [plan, setPlan] = useState<GuestkitMigratePlanReport | null>(null)
  const [busy, setBusy] = useState<'doctor' | 'plan' | null>(null)
  const [error, setError] = useState<string | null>(null)

  const stopped =
    vmState === 'stopped' || vmState === 'shut off' || vmState === 'shutoff' || vmState === 'Shutoff'

  useEffect(() => {
    if (!guestkitEnabled) return
    void getGuestkitStatus()
      .then((s) => setGkReachable(s.enabled))
      .catch(() => setGkReachable(false))
  }, [guestkitEnabled])

  const runDoctor = useCallback(async () => {
    setBusy('doctor')
    setError(null)
    try {
      const r = await guestkitVmDoctor(vmId, 'kvm', true)
      setDoctor(r)
    } catch (e: unknown) {
      setError(formatUserError(e))
      setDoctor(null)
    } finally {
      setBusy(null)
    }
  }, [vmId])

  const runMigratePlan = useCallback(async () => {
    setBusy('plan')
    setError(null)
    try {
      const r = await guestkitVmMigratePlan(vmId, 'kvm')
      setPlan(r)
    } catch (e: unknown) {
      setError(formatUserError(e))
      setPlan(null)
    } finally {
      setBusy(null)
    }
  }, [vmId])

  useEffect(() => {
    if (!guestkitEnabled || !autoRunMigratePlan) return
    void runMigratePlan()
  }, [guestkitEnabled, autoRunMigratePlan, runMigratePlan])

  if (!guestkitEnabled) {
    return (
      <div className={`rounded-xl border p-4 text-sm ${statusSurfaceClasses('neutral')}`}>
        <p className="font-medium text-[var(--text-primary)] flex items-center gap-2">
          <HardDrive className="w-4 h-4" />
          Offline disk assurance (GuestKit)
        </p>
        <p className="text-xs text-[var(--text-muted)] mt-2">
          Offline disk assurance isn't active on this controller. When an administrator enables it,
          stopped VM disks are scored for KVM migration — boot blockers, drivers, and readiness —
          without powering the guest on.
        </p>
        <p className="text-[10px] text-[var(--text-faint)] mt-1">
          Administrator setting: <span className="font-mono">GUESTKIT_ENABLED=1</span> on the controller.
        </p>
        <Link to="/platform/migration" className={`text-xs mt-2 inline-block ${hubLinkClasses()}`}>
          Migration hub →
        </Link>
      </div>
    )
  }

  return (
    <div className="rounded-xl border border-[var(--apple-hairline)] bg-orange-500/10 p-4 text-sm text-orange-900 space-y-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <p className="font-semibold text-orange-800 flex items-center gap-2">
            <HardDrive className="w-4 h-4" />
            Offline disk assurance (GuestKit)
          </p>
          <p className="text-xs text-orange-700/75 mt-1">
            Complements live QEMU guest-agent checks. Scans the VM disk image while{' '}
            {stopped ? 'the VM is stopped' : 'powered off or online'} — boot blockers, drivers, and migration score.
          </p>
        </div>
        {gkReachable !== null && (
          <span className={statusPillClasses(gkReachable ? 'ok' : 'warn')}>
            {gkReachable ? 'GuestKit enabled' : 'GuestKit off'}
          </span>
        )}
      </div>

      {stopped && (
        <p className="text-xs text-orange-700/90">
          VM is stopped — use GuestKit here; start the VM for live QGA health in the panel above.
        </p>
      )}

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="btn-secondary text-xs inline-flex items-center gap-1.5"
          disabled={busy !== null}
          onClick={() => void runDoctor()}
        >
          {busy === 'doctor' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Stethoscope className="w-3.5 h-3.5" />}
          Offline doctor
        </button>
        <button
          type="button"
          className="btn-secondary text-xs inline-flex items-center gap-1.5"
          disabled={busy !== null}
          onClick={() => void runMigratePlan()}
        >
          {busy === 'plan' ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Route className="w-3.5 h-3.5" />}
          Migrate plan
        </button>
        <Link to="/platform/migration" className={`btn-secondary text-xs ${hubLinkClasses()}`}>
          Migration hub
        </Link>
      </div>

      {error && <p className={`text-xs ${statusToneClass('error')}`}>{error}</p>}

      {(doctor || plan) && (
        <div className="grid gap-3 sm:grid-cols-2">
          {doctor && (
            <div className={`rounded-lg border p-3 text-xs ${statusSurfaceClasses(scoreTone(doctor.boot_score))}`}>
              <p className="font-medium">Boot assurance</p>
              <p className="mt-1 text-lg font-semibold tabular-nums">{doctor.boot_score.toFixed(0)}%</p>
              <p className="opacity-90 mt-1">{doctor.summary}</p>
              {doctor.blockers.length > 0 && (
                <ul className="mt-2 list-disc pl-4 space-y-0.5">
                  {doctor.blockers.slice(0, 4).map((b) => (
                    <li key={b}>{b}</li>
                  ))}
                </ul>
              )}
              {doctor.root_cause && (
                <p className="mt-2 opacity-80">Root cause: {doctor.root_cause}</p>
              )}
            </div>
          )}
          {plan && (
            <div className={`rounded-lg border p-3 text-xs ${statusSurfaceClasses(scoreTone(plan.migration_score))}`}>
              <p className="font-medium">KVM migration plan</p>
              <p className="mt-1 text-lg font-semibold tabular-nums">{plan.migration_score.toFixed(0)}%</p>
              <p className="opacity-90 mt-1">{plan.summary}</p>
              <p className="mt-1 opacity-75">~{plan.estimated_downtime_minutes} min estimated downtime</p>
              {plan.required_changes.length > 0 && (
                <ul className="mt-2 list-disc pl-4 space-y-0.5">
                  {plan.required_changes.slice(0, 4).map((c) => (
                    <li key={c}>{c}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      )}

      {!doctor && !plan && !error && (
        <p className="text-xs text-orange-700/60">
          Run offline doctor or migrate plan to inspect the VM disk without a running guest agent.
        </p>
      )}
    </div>
  )
}
