// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { AlertTriangle, Check, Loader2, Wand2 } from 'lucide-react'
import { getPlatformVm, runVmHealthCheck, vmPower, type PlatformVm } from '../../../api/platform'
import { daemonManagesVm, guestkitCapabilities, injectGuestAgent, type GuestkitCapabilities } from '../../../api/guestAgentInstall'
import { formatUserError } from '../../../utils/apiError'

type StepState = 'todo' | 'active' | 'done' | 'skipped' | 'failed'
type StepId = 'shutdown' | 'inject' | 'start' | 'verify'

const LABELS: Record<StepId, string> = {
  shutdown: 'Shut the VM down cleanly',
  inject: 'Install the agent into its disk (GuestKit)',
  start: 'Start the VM again',
  verify: 'Confirm the agent answers',
}

const OFF = (s: string) => /^(shutoff|shut off|stopped|off)$/i.test(s.trim())
const ON = (s: string) => /^(running)$/i.test(s.trim())
const AGENT_OK = (s?: string | null) => /^(ok|running|active|ready|installed|connected)/i.test(s ?? '')
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
 * One-click agent install for Linux guests: clean shutdown → GuestKit offline inject → start → verify.
 * It never force-stops a guest; if the clean shutdown does not finish it stops and says so.
 */
export default function GuestAgentAutoInstall({ vm, onFinished }: { vm: PlatformVm; onFinished?: () => void }) {
  const [available, setAvailable] = useState<boolean | null>(null)
  const [caps, setCaps] = useState<GuestkitCapabilities | null>(null)
  const [confirmed, setConfirmed] = useState(false)
  const [running, setRunning] = useState(false)
  const [steps, setSteps] = useState<Record<StepId, StepState>>({ shutdown: 'todo', inject: 'todo', start: 'todo', verify: 'todo' })
  const [error, setError] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [finished, setFinished] = useState(false)

  useEffect(() => {
    let alive = true
    void Promise.all([daemonManagesVm(vm.name), guestkitCapabilities()]).then(([managed, c]) => { if (!alive) return; setCaps(c); setAvailable(managed && Boolean(c?.cli_found && c.agent_binary_found)) })
    return () => { alive = false }
  }, [vm.name])

  const set = (id: StepId, s: StepState) => setSteps((prev) => ({ ...prev, [id]: s }))

  const run = async () => {
    setRunning(true)
    setError(null)
    setNote(null)
    let shutDownByUs = false
    try {
      const current = await getPlatformVm(vm.id)
      const wasOn = ON(current.observed_state)

      set('shutdown', 'active')
      if (wasOn) {
        await vmPower(vm.id, 'shutdown')
        const off = await waitFor(async () => OFF((await getPlatformVm(vm.id)).observed_state), 150_000)
        if (!off) throw new Error('The guest did not shut down within 2.5 minutes. Nothing was changed. Shut it down yourself, then try again.')
        shutDownByUs = true
      }
      set('shutdown', wasOn ? 'done' : 'skipped')

      set('inject', 'active')
      const report = await injectGuestAgent(vm.name)
      setNote(report.output ? report.output.split('\n').slice(-3).join('\n') : null)
      set('inject', 'done')

      set('start', 'active')
      if (wasOn || shutDownByUs) {
        await vmPower(vm.id, 'start')
        const up = await waitFor(async () => ON((await getPlatformVm(vm.id)).observed_state), 90_000)
        if (!up) throw new Error('The agent was installed, but the VM did not report running after 90 seconds. Check it in Machine Finder.')
        shutDownByUs = false
        set('start', 'done')
      } else {
        set('start', 'skipped')
      }

      set('verify', 'active')
      const ok = await waitFor(async () => AGENT_OK((await runVmHealthCheck(vm.id)).guest_tools_status), 75_000, 5000)
      set('verify', ok ? 'done' : 'skipped')
      if (!ok) setNote('Installed and started. The agent can take another minute to answer — refresh health shortly.')
      setFinished(true)
      onFinished?.()
    } catch (e: unknown) {
      setSteps((prev) => {
        const next = { ...prev }
        for (const k of Object.keys(next) as StepId[]) if (next[k] === 'active') next[k] = 'failed'
        return next
      })
      let msg = formatUserError(e)
      if (shutDownByUs) {
        // Do not leave a VM we powered off sitting off after a failed install.
        try { await vmPower(vm.id, 'start'); msg += ' The VM was started again.' } catch { msg += ' The VM is still powered off — start it from Machine Finder.' }
      }
      setError(msg)
    } finally {
      setRunning(false)
    }
  }

  if (available === null) {
    return <p className="flex items-center gap-2 text-xs text-[var(--text-muted)]"><Loader2 className="h-3.5 w-3.5 animate-spin" /> Checking whether this host can install automatically…</p>
  }
  if (!available) {
    return (
      <p className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]/40 px-3 py-2.5 text-xs text-[var(--text-muted)]">
        {caps && (!caps.cli_found || !caps.agent_binary_found)
          ? `Automatic install needs GuestKit on this hypervisor${!caps.cli_found ? ' (the guestkit command was not found)' : ` (agent binary not found at ${caps.agent_binary})`}. Install it, then this appears — or use one of the routes below.`
          : "Automatic install isn't available here — it needs the host that owns this VM to be the one you're connected to, with a current Machina daemon. Use one of the routes below."}
      </p>
    )
  }

  const started = Object.values(steps).some((s) => s !== 'todo')
  return (
    <div className="rounded-2xl border border-[color-mix(in_srgb,#0a84ff_32%,transparent)] bg-[color-mix(in_srgb,#0a84ff_7%,transparent)] p-4" data-testid="guest-agent-auto-install">
      <p className="flex items-center gap-2 font-medium text-[var(--text-primary)]"><Wand2 className="h-4 w-4 text-[#0a84ff]" /> Install automatically</p>
      <p className="mt-1 text-xs text-[var(--text-secondary)]">
        Machina shuts the VM down cleanly, writes the agent into its disk with GuestKit, starts it again and checks it answers. Linux guests only;
        expect a few minutes of downtime.
      </p>

      {!started ? (
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
            <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
            I understand {vm.name} will be restarted.
          </label>
          <button type="button" className="btn-primary text-sm" disabled={!confirmed || running} onClick={() => void run()}>
            Install agent now
          </button>
        </div>
      ) : (
        <ol className="mt-3 space-y-1.5">
          {(Object.keys(LABELS) as StepId[]).map((id) => {
            const s = steps[id]
            return (
              <li key={id} className="flex items-center gap-2 text-sm" data-step={id} data-state={s}>
                <span className="grid h-5 w-5 place-items-center rounded-full border border-[var(--apple-hairline)]" aria-hidden>
                  {s === 'done' ? <Check className="h-3 w-3 text-emerald-500" /> : s === 'active' ? <Loader2 className="h-3 w-3 animate-spin text-[#0a84ff]" /> : s === 'failed' ? <AlertTriangle className="h-3 w-3 text-red-500" /> : null}
                </span>
                <span className={s === 'todo' || s === 'skipped' ? 'text-[var(--text-muted)]' : 'text-[var(--text-primary)]'}>{LABELS[id]}{s === 'skipped' ? ' — skipped' : ''}</span>
              </li>
            )
          })}
        </ol>
      )}

      {note ? <pre className="mt-3 overflow-x-auto rounded-lg bg-[var(--apple-fill-tertiary)]/50 px-3 py-2 font-mono text-[11px] text-[var(--text-secondary)]">{note}</pre> : null}
      {error ? <p className="mt-3 text-xs text-red-500" role="alert">{error}</p> : null}
      {finished && !error ? <p className="mt-3 text-xs text-emerald-600">Done — the agent is installed.</p> : null}
      {error && !running ? (
        <button type="button" className="btn-secondary mt-3 text-xs" onClick={() => { setSteps({ shutdown: 'todo', inject: 'todo', start: 'todo', verify: 'todo' }); setError(null); setNote(null) }}>Try again</button>
      ) : null}
    </div>
  )
}
