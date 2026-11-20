// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

export type JoinEvent = {
  id: string
  level: 'info' | 'ok' | 'warn' | 'error' | string
  step: string
  message: string
  created_at: string
}

export type JoinHost = {
  id: string
  hostname: string
  address: string
  state: string
  validation_status: string
  vm_count: number
  cpu_percent: number
}

export type JoinProgress = {
  token_status: 'waiting' | 'joined' | 'expired' | string
  expires_at?: string | null
  host?: JoinHost | null
  events: JoinEvent[]
}

export type StageStatus = 'done' | 'active' | 'pending' | 'error'
export type JoinStage = { id: string; label: string; status: StageStatus }

const ORDER: { id: string; label: string }[] = [
  { id: 'token', label: 'Token issued' },
  { id: 'contact', label: 'Host contacted' },
  { id: 'register', label: 'Registered' },
  { id: 'validate', label: 'Validated' },
  { id: 'online', label: 'Online' },
]

/** Where the join is, derived from the controller's event log and the host's live state. */
export function joinStages(p: JoinProgress | null): JoinStage[] {
  const ev = p?.events ?? []
  const has = (step: string, level?: string) => ev.some((e) => e.step === step && (!level || e.level === level))
  const failed = ev.some((e) => e.step === 'validate' && e.level === 'error') || ev.some((e) => e.step === 'token' && e.level === 'error')
  const validated = ev.some((e) => e.step === 'validate' && e.level === 'ok')
  const online = Boolean(validated && p?.host && p.host.state === 'online')
  const done: Record<string, boolean> = {
    token: has('token') || (p != null && p.token_status !== 'expired'),
    contact: has('contact'),
    register: has('register'),
    validate: validated,
    online,
  }
  let activeAssigned = false
  return ORDER.map(({ id, label }) => {
    if (done[id]) return { id, label, status: 'done' as const }
    if (!activeAssigned) {
      activeAssigned = true
      return { id, label, status: failed && (id === 'validate' || id === 'online') ? ('error' as const) : ('active' as const) }
    }
    return { id, label, status: 'pending' as const }
  })
}

export type TermTone = 'info' | 'ok' | 'warn' | 'error' | 'dim'
export type TermLine = { key: string; time: string; symbol: string; text: string; tone: TermTone }

const SYMBOL: Record<string, string> = { ok: '✓', info: '›', warn: '!', error: '✗' }

/** `2026-10-07 12:34:56.789` → `12:34:56.789`. */
export function clockOf(createdAt: string): string {
  const m = /(\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?)/.exec(createdAt)
  return m ? m[1] : createdAt
}

export function terminalLines(events: JoinEvent[]): TermLine[] {
  return events.map((e) => {
    const tone: TermTone = e.level === 'ok' || e.level === 'warn' || e.level === 'error' ? e.level : 'info'
    return { key: e.id, time: clockOf(e.created_at), symbol: SYMBOL[tone] ?? '›', text: e.message, tone }
  })
}

/** The token is shown only as its first characters. */
export function maskToken(command: string, token: string): string {
  return token ? command.split(token).join(`${token.slice(0, 9)}••••`) : command
}

const stampMs = (createdAt: string): number => Date.parse(`${createdAt.replace(' ', 'T')}Z`)

/** Seconds after the first logged event at which each stage first appeared (stages with no event are left out). */
export function stageElapsed(events: JoinEvent[]): Record<string, number> {
  const times = events.map((e) => ({ step: e.step, t: stampMs(e.created_at) })).filter((e) => Number.isFinite(e.t))
  if (times.length === 0) return {}
  const start = Math.min(...times.map((e) => e.t))
  const out: Record<string, number> = {}
  for (const { step, t } of times) if (!(step in out)) out[step] = Math.max(0, Math.round((t - start) / 1000))
  return out
}

/** The failed validation checks the controller logged, as `name: message` strings. */
export function failedChecks(events: JoinEvent[]): string[] {
  return events.filter((e) => e.step === 'check' && e.level === 'error').map((e) => e.message)
}
