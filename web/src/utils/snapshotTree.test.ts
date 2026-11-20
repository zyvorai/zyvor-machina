// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, expect, it } from 'vitest'
import type { SnapshotInfo } from '../api/snapshot'
import { snapshotForest } from './snapshotTree'

function snap(name: string, parent: string): SnapshotInfo {
  return {
    name,
    vm_name: 'vm1',
    creation_time: 0,
    state: 'running',
    description: '',
    parent,
    is_current: false,
  }
}

describe('snapshotForest', () => {
  it('builds a tree from parent links', () => {
    const forest = snapshotForest([snap('root', ''), snap('child', 'root')])
    expect(forest).toHaveLength(1)
    expect(forest[0].snap.name).toBe('root')
    expect(forest[0].children).toHaveLength(1)
    expect(forest[0].children[0].snap.name).toBe('child')
  })

  it('treats a self-parented snapshot as a root instead of looping forever', () => {
    const forest = snapshotForest([snap('loopy', 'loopy')])
    expect(forest).toHaveLength(1)
    expect(forest[0].children).toHaveLength(0)
  })

  it('breaks a multi-node parent cycle (A -> B -> A) instead of hanging', () => {
    const forest = snapshotForest([snap('a', 'b'), snap('b', 'a')])
    // Both nodes are part of the cycle — each is demoted to a root rather
    // than nested inside the other, which would recurse forever on render.
    expect(forest).toHaveLength(2)
    for (const node of forest) {
      expect(node.children).toHaveLength(0)
    }
  })
})
