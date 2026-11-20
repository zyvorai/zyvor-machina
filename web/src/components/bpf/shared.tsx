// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Small helpers shared by the native data plane tabs.

import { useCallback, useEffect, useState, type ReactNode } from 'react'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'

export function fmtBytes(n: number): string {
  if (n >= 1 << 30) return `${(n / (1 << 30)).toFixed(1)} GiB`
  if (n >= 1 << 20) return `${(n / (1 << 20)).toFixed(1)} MiB`
  if (n >= 1 << 10) return `${(n / (1 << 10)).toFixed(1)} KiB`
  return `${n} B`
}

export function fmtUs(us: number): string {
  if (us >= 1_000_000) return `${(us / 1_000_000).toFixed(2)} s`
  if (us >= 1000) return `${(us / 1000).toFixed(1)} ms`
  return `${us} µs`
}

export const PROTO: Record<number, string> = { 6: 'tcp', 17: 'udp', 132: 'sctp' }

/** Comma / newline separated list → trimmed non-empty strings. */
export const splitList = (s: string) => s.split(/[\n,]/).map((x) => x.trim()).filter(Boolean)

/** Comma separated ports → valid port numbers. */
export const splitPorts = (s: string) =>
  splitList(s).map(Number).filter((n) => Number.isInteger(n) && n > 0 && n < 65536)

/** Load on mount and on `reload()`; errors become a message, not a toast. */
export function useBpfLoad<T>(load: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const reload = useCallback(async () => {
    try {
      setData(await load())
      setError(null)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [load])
  useEffect(() => { void reload() }, [reload])
  return { data, setData, error, reload }
}

/** Run a mutation with success / error toasts. */
export function useBpfAction() {
  const toast = useToastContext()
  return useCallback(
    async <T,>(p: Promise<T>, ok: string): Promise<T | undefined> => {
      try {
        const r = await p
        toast.success(ok)
        return r
      } catch (e: unknown) {
        toast.error(formatUserError(e))
        return undefined
      }
    },
    [toast],
  )
}

export function Metrics({ items }: { items: Array<{ label: string; value: ReactNode }> }) {
  return (
    <div className="apple-metric-band">
      {items.map((m) => (
        <div key={m.label} className="min-w-0">
          <div className="apple-metric-value">{m.value}</div>
          <div className="apple-metric-label">{m.label}</div>
        </div>
      ))}
    </div>
  )
}

export function Field({ label, htmlFor, children }: { label: string; htmlFor: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-xs text-[var(--text-muted)]" htmlFor={htmlFor}>{label}</label>
      {children}
    </div>
  )
}

export const Empty = ({ children }: { children: ReactNode }) => (
  <p className="text-sm text-[var(--text-muted)]">{children}</p>
)

export const thCls = 'py-2 pr-2'
export const headRowCls = 'text-left text-[var(--text-muted)] border-b border-white/[0.06]'
export const rowCls = 'border-b border-white/[0.04]'
