// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import { workloadLabel } from '../../api/bpf'
import { fmtUs, splitList, splitPorts } from './shared'

describe('native bpf helpers', () => {
  it('splits lists and ports', () => {
    expect(splitList(' 10.0.0.0/8,\n\n192.168.0.0/16 ')).toEqual(['10.0.0.0/8', '192.168.0.0/16'])
    expect(splitPorts('22, 6443, 0, 70000, x, 5092')).toEqual([22, 6443, 5092])
  })

  it('labels workloads', () => {
    expect(workloadLabel({ kind: 'pod', ns: 'prod', name: 'web-0' })).toBe('pod prod/web-0')
    expect(workloadLabel({ kind: 'vm', name: 'db' })).toBe('vm db')
    expect(workloadLabel(null)).toBe('')
  })

  it('formats microseconds', () => {
    expect(fmtUs(250)).toBe('250 µs')
    expect(fmtUs(4200)).toBe('4.2 ms')
    expect(fmtUs(1_500_000)).toBe('1.50 s')
  })
})
