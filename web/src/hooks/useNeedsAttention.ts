// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { getFleetFinder } from '../api/platform'

/** Number of machines in the Machine Finder "needs attention" smart folder (0 while loading or on error). */
export function useNeedsAttention(): number {
  const [count, setCount] = useState(0)
  useEffect(() => {
    let live = true
    void getFleetFinder()
      .then((f) => { if (live) setCount(f?.smart_folders.find((x) => x.id === 'needs_attention')?.count ?? 0) })
      .catch(() => { if (live) setCount(0) })
    return () => { live = false }
  }, [])
  return count
}
