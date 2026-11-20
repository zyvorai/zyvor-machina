// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState } from 'react'

/** Animates a number toward `target` over `ms` (ease-out). Jumps straight there with reduced motion. */
export function useCountUp(target: number | null, ms = 600): number | null {
  const [value, setValue] = useState<number | null>(target)
  const from = useRef<number>(0)
  useEffect(() => {
    if (target == null) { setValue(null); return }
    const reduce = typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
    if (reduce || ms <= 0) { setValue(target); from.current = target; return }
    const start = performance.now()
    const a = from.current
    let raf = 0
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / ms)
      const e = 1 - Math.pow(1 - t, 3)
      setValue(a + (target - a) * e)
      if (t < 1) raf = requestAnimationFrame(tick)
      else from.current = target
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [target, ms])
  return value
}
