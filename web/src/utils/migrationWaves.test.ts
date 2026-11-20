// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { blockers, planWaves, type ScoredMachine } from './migrationWaves'

const plan = (over: Partial<NonNullable<ScoredMachine['plan']>> = {}) => ({
  image_path: '/d', target: 'kvm', migration_score: 90, boot_score: 90, estimated_downtime_minutes: 5,
  driver_injections: [], required_changes: [], licensing_warnings: [], summary: '', ...over,
})
const m = (name: string, over?: Partial<NonNullable<ScoredMachine['plan']>>): ScoredMachine => ({ id: name, name, plan: plan(over) })

describe('planWaves', () => {
  it('puts high scores in wave 1, mid in wave 2, low in wave 3', () => {
    const { waves } = planWaves([m('a', { migration_score: 92 }), m('b', { migration_score: 75 }), m('c', { migration_score: 40 })])
    expect(waves.map((w) => w.machines.map((x) => x.name))).toEqual([['a'], ['b'], ['c']])
  })

  it('a licensing warning or poor boot score pushes a high-scoring machine to wave 3', () => {
    const { waves } = planWaves([m('lic', { licensing_warnings: ['OEM key'] }), m('boot', { boot_score: 40 })])
    expect(waves[2].machines.map((x) => x.name).sort()).toEqual(['boot', 'lic'])
    expect(blockers(plan({ licensing_warnings: ['OEM key'] }))).toEqual(['Licensing: OEM key'])
  })

  it('quickest cutovers go first and downtime is summed per wave', () => {
    const { waves } = planWaves([m('slow', { estimated_downtime_minutes: 30 }), m('fast', { estimated_downtime_minutes: 3 })])
    expect(waves[0].machines.map((x) => x.name)).toEqual(['fast', 'slow'])
    expect(waves[0].downtimeMinutes).toBe(33)
  })

  it('keeps machines GuestKit could not score out of the waves', () => {
    const { waves, unscored } = planWaves([m('ok'), { id: 'x', name: 'x', plan: null, error: 'not on this host' }])
    expect(waves.flatMap((w) => w.machines)).toHaveLength(1)
    expect(unscored.map((u) => u.name)).toEqual(['x'])
  })
})
