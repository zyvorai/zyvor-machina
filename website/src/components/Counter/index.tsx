// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import {useEffect, useRef, useState} from 'react';
import type {ReactNode} from 'react';

type CounterProps = {
  to: number;
  suffix?: string;
  durationMs?: number;
};

/** Counts up from 0 once scrolled into view; renders the final value without JS or with reduced motion. */
export default function Counter({to, suffix = '', durationMs = 1400}: CounterProps): ReactNode {
  const ref = useRef<HTMLSpanElement>(null);
  const [value, setValue] = useState(to);

  useEffect(() => {
    const node = ref.current;
    const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (!node || reduced || typeof IntersectionObserver === 'undefined') {
      return;
    }
    setValue(0);
    let frame = 0;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry.isIntersecting) return;
        observer.disconnect();
        const start = performance.now();
        const tick = (now: number) => {
          const t = Math.min(1, (now - start) / durationMs);
          const eased = 1 - Math.pow(1 - t, 3);
          setValue(Math.round(eased * to));
          if (t < 1) frame = requestAnimationFrame(tick);
        };
        frame = requestAnimationFrame(tick);
      },
      {threshold: 0.4},
    );
    observer.observe(node);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [to, durationMs]);

  return (
    <span ref={ref}>
      {value}
      {suffix}
    </span>
  );
}
