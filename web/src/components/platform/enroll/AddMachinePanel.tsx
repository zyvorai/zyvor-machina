// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { AlertTriangle, CheckCircle2, RefreshCw, ShieldCheck } from 'lucide-react'
import CopyButton from '../../CopyButton'
import UnderlineTabs from '../../kit/UnderlineTabs'
import JoinLivePanel from '../JoinLivePanel'
import {
  createEnrollmentToken,
  enqueueValidateHost,
  getPlatformHostDetail,
  type EnrollmentToken,
} from '../../../api/platform'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import { failedChecks, joinStages, type JoinProgress } from '../../../utils/joinProgress'
import { commandOf, formatLeft, listenerOff, secondsLeft, snippets } from './enrollCommand'

type OsTab = 'debian' | 'rhel'
type Section = 'command' | 'automation'
type Problem = { name: string; message: string; remediation?: string }

const OS_TABS: Array<{ id: OsTab; label: string }> = [
  { id: 'debian', label: 'Ubuntu / Debian' },
  { id: 'rhel', label: 'RHEL family' },
]
const OS_NOTES: Record<OsTab, string> = {
  debian: 'The installer uses apt-get: libvirt-daemon-system, libvirt-clients, qemu-system-x86 (qemu-system-arm on ARM) and qemu-utils.',
  rhel: 'The installer uses dnf: libvirt, libvirt-client and qemu-kvm (openSUSE: zypper with libvirt and qemu-kvm).',
}
const TTLS = [1, 24, 72]

type Props = {
  /** Called whenever a token is created, so a page can refresh its token list. */
  onTokenCreated?: (t: EnrollmentToken) => void
  /** Terminal height; the page uses the roomy default, the modal passes less. */
  terminalHeight?: number
}

/** Add a machine: one command, a live view of the join, and what to do when it fails. Shared by the Enroll page and the wizard. */
export default function AddMachinePanel({ onTokenCreated, terminalHeight = 360 }: Props) {
  const toast = useToastContext()
  const [token, setToken] = useState<EnrollmentToken | null>(null)
  const [ttl, setTtl] = useState(24)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [os, setOs] = useState<OsTab>('debian')
  const [section, setSection] = useState<Section>('command')
  const [progress, setProgress] = useState<JoinProgress | null>(null)
  const [checkRun, setCheckRun] = useState(0)
  const [problems, setProblems] = useState<Problem[]>([])
  const [now, setNow] = useState(() => Date.now())
  const started = useRef(false)

  const generate = useCallback(async (hours: number) => {
    setBusy(true)
    setError(null)
    try {
      const t = await createEnrollmentToken(hours)
      setToken(t)
      setProgress(null)
      setProblems([])
      setCheckRun(0)
      onTokenCreated?.(t)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }, [onTokenCreated])

  useEffect(() => {
    if (started.current) return
    started.current = true
    void generate(ttl)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(t)
  }, [])

  const stages = useMemo(() => joinStages(progress), [progress])
  const complete = progress != null && stages.every((s) => s.status === 'done')
  const failed = stages.some((s) => s.status === 'error')
  const host = progress?.host ?? null
  const events = progress?.events
  const left = secondsLeft(token?.expires_at, now)
  const command = token ? commandOf(token) : ''
  const off = token ? listenerOff(token) : false
  const controllerUrl = /--controller\s+(\S+)/.exec(command)?.[1]

  // When validation failed, explain it: the host's own checklist has the remediation text.
  useEffect(() => {
    if (!failed) { setProblems([]); return }
    let stop = false
    void (async () => {
      let found: Problem[] = []
      if (host) {
        try {
          const d = await getPlatformHostDetail(host.id)
          found = (d.validation_report ?? []).filter((c) => !c.passed).map((c) => ({ name: c.name, message: c.message, remediation: c.remediation }))
        } catch { /* fall back to the log */ }
      }
      if (found.length === 0) found = failedChecks(events ?? []).map((m) => ({ name: m.split(':')[0], message: m }))
      if (!stop) setProblems(found)
    })()
    return () => { stop = true }
  }, [failed, host, events])

  const recheck = async () => {
    if (!host) return
    try {
      await enqueueValidateHost(host.id)
      toast.success('Re-checking the host')
      setCheckRun((n) => n + 1)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const code = useMemo(() => (command ? snippets(command) : null), [command])

  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]" data-testid="add-machine-panel">
      <div className="space-y-4 min-w-0">
        <UnderlineTabs
          label="How to add a machine"
          value={section}
          onChange={setSection}
          tabs={[{ id: 'command', label: 'Run a command' }, { id: 'automation', label: 'Automation' }]}
        />

        {error && <p role="alert" className="text-sm text-[var(--nl-danger,#ff453a)]">{error}</p>}

        {section === 'command' && (
          <div className="space-y-4">
            {off && (
              <div role="alert" data-testid="listener-off-banner" className="rounded-xl border border-[var(--nl-status-warn-border,#ffd60a)] bg-[var(--nl-status-warn-bg,rgba(255,214,10,.12))] p-3 text-sm space-y-1">
                <div className="flex items-center gap-2 font-medium"><AlertTriangle className="w-4 h-4" aria-hidden /> The HTTPS join listener is off</div>
                <p className="text-[var(--text-secondary)]">
                  The command below only works on the controller itself. To add other machines, set <code>MACHINA_CONTROLLER_TLS_ADDR=0.0.0.0:5094</code> in
                  {' '}<code>/etc/default/machina-platform</code>, run <code>sudo systemctl restart machina-controller</code> and <code>sudo machinactl dist publish</code>, then generate a new token.
                </p>
              </div>
            )}

            <UnderlineTabs label="Operating system" value={os} onChange={setOs} tabs={OS_TABS} />
            <p className="text-sm text-[var(--text-secondary)]">{OS_NOTES[os]}</p>

            <div>
              <div className="mb-1 flex items-center justify-between gap-2 text-sm text-[var(--text-muted)]">
                <span>{off ? 'Local-only command' : 'Run this on the new machine as root'}</span>
                {left != null && <span data-testid="token-countdown" className="tabular-nums">{formatLeft(left)}</span>}
              </div>
              <div className="flex items-start gap-2">
                <pre data-testid="join-command" className="flex-1 whitespace-pre-wrap break-all rounded-lg bg-[var(--apple-surface)] p-3 text-xs text-[var(--text-secondary)]">{command || (busy ? 'Creating a token…' : '')}</pre>
                {command && <CopyButton text={command} label="Copy" />}
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-3 text-sm">
              <label className="inline-flex items-center gap-2 text-[var(--text-secondary)]">
                Valid for
                <select aria-label="Token lifetime" className="rounded-md border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-2 py-1" value={ttl} onChange={(e) => setTtl(Number(e.target.value))}>
                  {TTLS.map((h) => <option key={h} value={h}>{h} h</option>)}
                </select>
              </label>
              <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1.5" disabled={busy} onClick={() => void generate(ttl)}>
                <RefreshCw className="w-3.5 h-3.5" aria-hidden /> {busy ? 'Creating…' : 'New token'}
              </button>
            </div>

            <ul className="space-y-1 text-sm text-[var(--text-secondary)]" aria-label="Before you run it">
              <li>• Root or sudo on the machine, with <code>/dev/kvm</code> (virtualisation enabled).</li>
              <li>• The machine can reach {controllerUrl ? <code>{controllerUrl}</code> : 'the controller'} (the command pins its CA fingerprint).</li>
              <li>• The controller has published the agent: <code>sudo machinactl dist publish</code>.</li>
              <li>• One token joins one machine, once.</li>
            </ul>
          </div>
        )}

        {section === 'automation' && code && (
          <div className="space-y-4" data-testid="automation-tab">
            <p className="text-sm text-[var(--text-secondary)]">
              Every route ends in the same join command. Files are in <code>deploy/</code>; the walkthrough is <code>docs/QUICKSTART.md</code>.
            </p>
            {([['Ansible', code.ansible], ['cloud-init', code.cloudInit], ['Terraform / OpenTofu', code.terraform]] as const).map(([name, text]) => (
              <div key={name}>
                <div className="mb-1 flex items-center justify-between text-sm font-medium">{name}<CopyButton text={text} label="Copy" /></div>
                <pre className="overflow-x-auto rounded-lg bg-[var(--apple-surface)] p-3 text-xs text-[var(--text-secondary)]">{text}</pre>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="space-y-4 min-w-0">
        {complete && host && (
          <div role="status" data-testid="join-success" className="flex flex-wrap items-center gap-3 rounded-xl border border-[var(--nl-status-ok-border,#30d158)] bg-[var(--nl-status-ok-bg,rgba(48,209,88,.12))] p-3">
            <CheckCircle2 className="w-5 h-5 text-[var(--nl-accent-green-text,#30d158)]" aria-hidden />
            <div className="flex-1 text-sm">
              <div className="font-medium">{host.hostname} joined and passed validation</div>
              <div className="text-[var(--text-secondary)] inline-flex items-center gap-1"><ShieldCheck className="w-3.5 h-3.5" aria-hidden /> {host.address}</div>
            </div>
            <Link to={`/platform/hosts/${host.id}`} className="btn-secondary text-xs">Open host</Link>
            <button type="button" className="btn-primary text-xs" disabled={busy} onClick={() => void generate(ttl)}>Add another</button>
          </div>
        )}

        {failed && (
          <div role="alert" data-testid="join-failed" className="space-y-2 rounded-xl border border-[var(--nl-status-danger-border,#ff453a)] bg-[var(--nl-status-danger-bg,rgba(255,69,58,.12))] p-3 text-sm">
            <div className="font-medium">The host registered, but validation failed</div>
            {problems.length === 0 && <p className="text-[var(--text-secondary)]">See the red lines in the log below.</p>}
            <ul className="space-y-1">
              {problems.map((p) => (
                <li key={p.name}>
                  <span className="font-medium">{p.name}</span>: {p.message}
                  {p.remediation && <div className="text-[var(--text-secondary)]">Fix: {p.remediation}</div>}
                </li>
              ))}
            </ul>
            {host && <button type="button" className="btn-primary text-xs" onClick={() => void recheck()}>Re-check</button>}
          </div>
        )}

        {token ? (
          <JoinLivePanel key={`${token.token}-${checkRun}`} token={token.token} command={command} terminalHeight={terminalHeight} onProgress={setProgress} />
        ) : (
          <p className="text-sm text-[var(--text-muted)]">{busy ? 'Creating a token…' : 'No token yet.'}</p>
        )}
      </div>
    </div>
  )
}
