// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { apiPost } from './client'
import { guestkitCapabilities, type GuestkitCapabilities } from './guestAgentInstall'

/** Result of an offline GuestKit diagnose/repair on a powered-off VM's disk (daemon on the VM's host). */
export type GuestRepairReport = {
  vm: string
  disk: string
  action: 'diagnose' | 'repair'
  dry_run: boolean
  backup: boolean
  /** GuestKit output; `diagnose` asks for JSON. */
  output: string
  exit_ok: boolean
}

export { guestkitCapabilities }
export type { GuestkitCapabilities }

const base = (name: string) => `/api/v1/vms/${encodeURIComponent(name)}/guest-repair`

export const diagnoseGuestDisk = (name: string) => apiPost<GuestRepairReport>(`${base(name)}/diagnose`, {})

export const repairGuestDisk = (name: string, opts: { dryRun: boolean; backup?: boolean }) =>
  apiPost<GuestRepairReport>(`${base(name)}/apply`, { dry_run: opts.dryRun, backup: opts.backup ?? true })

export const driftGuestDisk = (name: string, baseline: string) =>
  apiPost<GuestRepairReport>(`/api/v1/vms/${encodeURIComponent(name)}/guest-drift`, { baseline })

/** Pull a score and readable findings out of GuestKit's doctor JSON (it nests them under `bootability`). */
export function summariseDoctorOutput(output: string): { score: number | null; findings: string[]; raw: string } {
  let data: unknown = null
  try {
    data = JSON.parse(output)
  } catch {
    // GuestKit may print text before/after the JSON — try the outermost braces.
    const a = output.indexOf('{')
    const b = output.lastIndexOf('}')
    if (a >= 0 && b > a) {
      try { data = JSON.parse(output.slice(a, b + 1)) } catch { data = null }
    }
  }
  if (!data || typeof data !== 'object') return { score: null, findings: [], raw: output }
  const root = data as Record<string, unknown>
  const nested = root.bootability && typeof root.bootability === 'object' ? (root.bootability as Record<string, unknown>) : null
  const sources = nested ? [nested, root] : [root]

  let score: number | null = null
  for (const obj of sources) {
    const key = ['boot_score', 'score', 'bootScore'].find((k) => typeof obj[k] === 'number')
    if (key) {
      score = Math.max(0, Math.min(100, Math.round(obj[key] as number)))
      break
    }
  }

  const findings: string[] = []
  const take = (v: unknown) => {
    if (typeof v === 'string') findings.push(v)
    else if (v && typeof v === 'object') {
      const o = v as Record<string, unknown>
      const title = typeof o.title === 'string' ? o.title : null
      const msg = [o.message, o.description, o.summary, o.detail].find((x) => typeof x === 'string') as string | undefined
      if (title && msg && msg !== title) findings.push(`${title}: ${msg}`)
      else if (title || msg) findings.push((title ?? msg) as string)
    }
  }
  for (const obj of sources) {
    for (const key of ['blockers', 'warnings', 'findings', 'issues', 'root_causes', 'causes', 'recommendations']) {
      const v = obj[key]
      if (Array.isArray(v)) v.forEach(take)
    }
  }
  return { score, findings: [...new Set(findings)].slice(0, 12), raw: output }
}

/** GuestKit prints "Backup created: <path>" when asked to back the disk up first. */
export function extractBackupPath(output: string): string | null {
  const m = output.match(/Backup created:\s*(\S+)/)
  return m ? m[1] : null
}
