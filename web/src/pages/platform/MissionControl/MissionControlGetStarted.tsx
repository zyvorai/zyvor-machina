// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Check, X } from 'lucide-react'
import Reveal from '../../../components/Reveal'
import { listNotificationChannelRows } from '../../../api/day2'
import type { MissionControlFleetState } from './useMissionControlFleet'

const DISMISS_KEY = 'machina-get-started-dismissed'

type Step = { id: string; title: string; hint: string; done: boolean; to?: string; onClick?: () => void; cta: string }

function agentOk(status?: string | null): boolean {
  return /^(ok|running|active|ready|installed|connected)/i.test(status ?? '')
}

/** First-run checklist driven by real fleet state; disappears once everything is done (or is dismissed). */
export default function MissionControlGetStarted({ state, onCreateVm }: { state: MissionControlFleetState; onCreateVm: () => void }) {
  const [dismissed, setDismissed] = useState(() => {
    try { return localStorage.getItem(DISMISS_KEY) === '1' } catch { return false }
  })
  const { hosts, vms, unprotected, loading, error } = state
  // null = not known yet (do not nag before the answer arrives, and not when it cannot be read)
  const [hasChannel, setHasChannel] = useState<boolean | null>(null)
  useEffect(() => {
    let alive = true
    listNotificationChannelRows()
      .then((c) => { if (alive) setHasChannel(Array.isArray(c) && c.some((x) => x.enabled)) })
      .catch(() => { if (alive) setHasChannel(null) })
    return () => { alive = false }
  }, [])

  const steps = useMemo<Step[]>(() => {
    const running = vms.filter((v) => v.observed_state === 'running')
    const missingAgent = running.filter((v) => !agentOk(v.guest_tools_status)).length
    const haOn = vms.some((v) => v.ha_enabled)
    return [
      { id: 'host', title: 'Add a hypervisor', hint: 'Enroll the machine that will run your VMs.', done: hosts.length > 0, to: '/platform/enroll', cta: 'Add host' },
      { id: 'vm', title: 'Create your first VM', hint: 'Pick an image and size — about a minute.', done: vms.length > 0, onClick: onCreateVm, cta: 'Create VM' },
      {
        id: 'agent',
        title: 'Install the guest agent',
        hint: missingAgent > 0 ? `${missingAgent} running machine${missingAgent === 1 ? '' : 's'} without it — unlocks health, IP and graceful shutdown.` : 'Unlocks health, IP and graceful shutdown.',
        done: vms.length > 0 && running.length > 0 && missingAgent === 0,
        to: '/platform/vms?folder=guest_agent_missing',
        cta: 'Show machines',
      },
      { id: 'backup', title: 'Protect with backups', hint: unprotected > 0 ? `${unprotected} machine${unprotected === 1 ? ' has' : 's have'} no backup.` : 'Schedule backups so nothing is lost.', done: vms.length > 0 && unprotected === 0, to: '/platform/backups', cta: 'Set up backups' },
      ...(hasChannel === null ? [] : [{
        id: 'alerts',
        title: 'Get told when something breaks',
        hint: 'Alerts for offline hosts, full storage, failed backups and failed tasks are on by default. Add a Slack, email or webhook channel to receive them.',
        done: hasChannel,
        to: '/platform/alert-rules',
        cta: 'Add channel',
      }]),
      { id: 'ha', title: 'Keep machines running', hint: hosts.length < 2 ? 'Needs a second host — add one to enable failover.' : 'Restart VMs on another host if one fails.', done: haOn || hosts.length < 2, to: '/platform/ha', cta: 'High availability' },
    ]
  }, [hosts.length, vms, unprotected, onCreateVm, hasChannel])

  if (dismissed || loading || error) return null
  const doneCount = steps.filter((s) => s.done).length
  if (doneCount === steps.length) return null
  const next = steps.find((s) => !s.done)

  return (
    <Reveal>
      <section className="apple-section apple-section--tight" aria-label="Get started" data-testid="mission-control-get-started">
        <div className="nl-getstarted">
          <header className="flex flex-wrap items-start justify-between gap-3">
            <div className="min-w-0">
              <p className="apple-eyebrow">Get started</p>
              <h2 className="apple-display apple-display--sm">{doneCount} of {steps.length} done</h2>
            </div>
            <button
              type="button"
              className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 text-xs text-[var(--text-muted)] hover:bg-[var(--apple-fill-tertiary)] hover:text-[var(--text-primary)]"
              onClick={() => { try { localStorage.setItem(DISMISS_KEY, '1') } catch { /* private mode */ } setDismissed(true) }}
              aria-label="Dismiss the getting-started checklist"
            >
              <X className="h-3.5 w-3.5" /> Dismiss
            </button>
          </header>
          <div className="nl-getstarted-bar" role="progressbar" aria-label="Setup progress" aria-valuemin={0} aria-valuemax={steps.length} aria-valuenow={doneCount}>
            <i style={{ width: `${(doneCount / steps.length) * 100}%` }} />
          </div>
          <ol className="nl-getstarted-list">
            {steps.map((s) => {
              const isNext = next?.id === s.id
              const action = s.onClick ? (
                <button type="button" className={isNext ? 'btn-primary text-xs' : 'btn-secondary text-xs'} onClick={s.onClick}>{s.cta}</button>
              ) : (
                <Link to={s.to!} className={isNext ? 'btn-primary text-xs' : 'btn-secondary text-xs'}>{s.cta}</Link>
              )
              return (
                <li key={s.id} className="nl-getstarted-step" data-done={s.done ? 'true' : 'false'} data-next={isNext ? 'true' : 'false'}>
                  <span className="nl-getstarted-check" aria-hidden>{s.done ? <Check className="h-3.5 w-3.5" /> : null}</span>
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium text-[var(--text-primary)]">{s.title}</span>
                    <span className="block text-xs text-[var(--text-muted)]">{s.hint}</span>
                  </span>
                  {!s.done ? action : <span className="text-xs text-emerald-600">Done</span>}
                </li>
              )
            })}
          </ol>
        </div>
      </section>
    </Reveal>
  )
}
