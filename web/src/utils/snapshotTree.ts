// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import type { SnapshotInfo } from '../api/snapshot'

export type SnapshotTreeNode = { snap: SnapshotInfo; children: SnapshotTreeNode[] }

/** Build a forest from libvirt `parent` links (flat list → tree for UI). */
export function snapshotForest(snaps: SnapshotInfo[]): SnapshotTreeNode[] {
  const m = new Map<string, SnapshotTreeNode>()
  for (const s of snaps) {
    m.set(s.name, { snap: s, children: [] })
  }
  const parentOf = new Map<string, string>()
  for (const s of snaps) {
    const p = s.parent?.trim()
    if (p && p !== s.name && m.has(p)) parentOf.set(s.name, p)
  }

  // A self-parent or a parent cycle (A -> B -> A) would otherwise make a node
  // its own ancestor, so walking `children` recursively (tree rendering) never
  // terminates and hangs the tab. Treat any node whose ancestor chain loops
  // back on itself as a root instead of attaching it under its claimed parent.
  function inCycle(name: string): boolean {
    const seen = new Set<string>()
    let cur: string | undefined = name
    while (cur) {
      if (seen.has(cur)) return true
      seen.add(cur)
      cur = parentOf.get(cur)
    }
    return false
  }

  const roots: SnapshotTreeNode[] = []
  for (const s of snaps) {
    const n = m.get(s.name)!
    const p = parentOf.get(s.name)
    if (p && !inCycle(s.name)) {
      m.get(p)!.children.push(n)
    } else {
      roots.push(n)
    }
  }
  return roots
}
