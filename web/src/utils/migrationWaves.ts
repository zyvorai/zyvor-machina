// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { GuestkitMigratePlanReport } from '../api/guestkit'

export type ScoredMachine = {
  id: string
  name: string
  plan: GuestkitMigratePlanReport | null
  /** Set when GuestKit could not score this machine (e.g. it is not on this host). */
  error?: string
}

export type Wave = {
  id: 1 | 2 | 3
  title: string
  blurb: string
  machines: ScoredMachine[]
  downtimeMinutes: number
}

/** A machine blocks migration until someone acts when it has licensing warnings or a poor boot/migration score. */
export const blockers = (p: GuestkitMigratePlanReport): string[] => [
  ...p.licensing_warnings.map((w) => `Licensing: ${w}`),
  ...(p.boot_score < 60 ? [`Boot score is ${Math.round(p.boot_score)}% — fix the boot problem first`] : []),
]

/**
 * Group scored machines into cutover waves. Wave 1 = ready now (score ≥ 85, nothing blocking), wave 2 = ready with
 * known changes (70–84, nothing blocking), wave 3 = needs work first (below 70, or anything blocking).
 * Inside a wave the quickest cutovers go first. Machines GuestKit could not score are returned separately.
 */
export function planWaves(rows: ScoredMachine[]): { waves: Wave[]; unscored: ScoredMachine[] } {
  const scored = rows.filter((r) => r.plan)
  const unscored = rows.filter((r) => !r.plan)
  const bucket = (r: ScoredMachine): 1 | 2 | 3 => {
    const p = r.plan as GuestkitMigratePlanReport
    if (blockers(p).length > 0 || p.migration_score < 70) return 3
    return p.migration_score >= 85 ? 1 : 2
  }
  const meta: Record<1 | 2 | 3, Pick<Wave, 'title' | 'blurb'>> = {
    1: { title: 'Wave 1 — ready now', blurb: 'High score, nothing blocking. Move these first.' },
    2: { title: 'Wave 2 — ready with a few changes', blurb: 'Apply the listed changes (drivers, config), then move.' },
    3: { title: 'Wave 3 — needs work first', blurb: 'Fix what is blocking each machine before it is scheduled.' },
  }
  const waves = ([1, 2, 3] as const).map((id) => {
    const machines = scored
      .filter((r) => bucket(r) === id)
      .sort((a, b) => (a.plan!.estimated_downtime_minutes - b.plan!.estimated_downtime_minutes) || a.name.localeCompare(b.name))
    return {
      id,
      ...meta[id],
      machines,
      downtimeMinutes: machines.reduce((n, m) => n + m.plan!.estimated_downtime_minutes, 0),
    }
  })
  return { waves, unscored }
}
