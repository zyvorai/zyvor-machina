// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useRef } from 'react'
import { getJournalBoots, getJournalLogs, JournalBootEntry, JournalEntry } from '../api/extras'
import { RefreshCw, Search, X } from 'lucide-react'
import PageLayout from '../components/PageLayout'
import { formatUserError } from '../utils/apiError'
import { journalPriorityTone, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

const PRIORITIES = ['emerg', 'alert', 'crit', 'err', 'warning', 'notice', 'info', 'debug'] as const
const LINE_COUNTS = [50, 100, 500, 1000] as const
const BOOT_FILTERS = [
  { label: 'All boots', value: '' },
  { label: 'Current boot', value: '0' },
  { label: 'Previous boot', value: '-1' },
] as const

function priorityColor(p: string): string {
  return statusToneClass(journalPriorityTone(p))
}

function priorityBg(p: string): string {
  const tone = journalPriorityTone(p)
  if (tone === 'error' || tone === 'warn') {
    return statusSurfaceClasses(tone, 'border-l-2')
  }
  return ''
}

export default function LogsPage() {
  const [entries, setEntries] = useState<JournalEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [priority, setPriority] = useState('')
  const [unit, setUnit] = useState('')
  const [boot, setBoot] = useState('')
  const [boots, setBoots] = useState<JournalBootEntry[]>([])
  const [since, setSince] = useState('')
  const [until, setUntil] = useState('')
  const [grep, setGrep] = useState('')
  const [uid, setUid] = useState('')
  const [pid, setPid] = useState('')
  const [kernelOnly, setKernelOnly] = useState(false)
  const [lineCount, setLineCount] = useState(100)
  const [autoRefresh, setAutoRefresh] = useState(false)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  // Last-response-wins: rapid filter changes (unit/priority/search/etc.) can
  // leave stale requests in flight; only the newest request may commit so an
  // old filter's slow response can't overwrite the current filter's results.
  const loadSeq = useRef(0)

  const load = useCallback(async () => {
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    try {
      setLoading(true)
      setLoadError(null)
      const data = await getJournalLogs({
        lines: lineCount,
        priority: priority || undefined,
        unit: unit || undefined,
        boot: boot === '' ? undefined : Number(boot),
        since: since || undefined,
        until: until || undefined,
        grep: grep || undefined,
        uid: uid.trim() ? Number(uid) : undefined,
        pid: pid.trim() ? Number(pid) : undefined,
        kernel: kernelOnly,
      })
      if (alive()) setEntries(data)
    } catch (e: unknown) {
      if (alive()) setLoadError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [lineCount, priority, unit, boot, since, until, grep, uid, pid, kernelOnly])

  useEffect(() => { load() }, [load])

  useEffect(() => {
    getJournalBoots()
      .then(setBoots)
      .catch(() => setBoots([]))
  }, [])

  useEffect(() => {
    if (autoRefresh) {
      intervalRef.current = setInterval(load, 5000)
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current)
    }
  }, [autoRefresh, load])

  return (
    <PageLayout
      eyebrow="System"
      title="System Logs"
      subtitle="journald log viewer"
      actions={
        <button onClick={load} className="p-2 hover:bg-[var(--surface-hover)] rounded-lg transition" title="Refresh" aria-label="Refresh">
          <RefreshCw className="w-4 h-4" />
        </button>
      }
      error={loadError}
      errorTitle="Could not load journal logs"
      errorHints={[
        'Confirm machina-daemon is running and your session is valid.',
        'journalctl must be available on the host; check daemon logs if filters fail.',
      ]}
      onErrorRetry={load}
    >
      {/* Filters */}
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={() => {
            setUnit('sshd')
            setPriority('')
            setBoot('0')
            setSince('1 hour ago')
            setUntil('')
            setGrep('')
          }}
          className="px-3 py-2 text-xs rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] hover:bg-[var(--surface-hover)]/40"
        >
          SSH last 1h
        </button>
        <button
          type="button"
          onClick={() => {
            setUnit('')
            setPriority('err')
            setBoot('-1')
            setSince('')
            setUntil('')
            setGrep('')
          }}
          className="px-3 py-2 text-xs rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] hover:bg-[var(--surface-hover)]/40"
        >
          Previous boot errors
        </button>

        <select
          aria-label="Priority"
          value={priority}
          onChange={e => setPriority(e.target.value)}
          className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        >
          <option value="">All priorities</option>
          {PRIORITIES.map(p => (
            <option key={p} value={p}>{p}</option>
          ))}
        </select>

        <div className="relative flex-1 min-w-[200px] max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
          <input
            type="text"
            aria-label="Filter by unit"
            placeholder="Filter by unit (e.g. sshd, machina-daemon)"
            value={unit}
            onChange={e => setUnit(e.target.value)}
            className={`w-full pl-10 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)] ${unit ? 'pr-8' : 'pr-4'}`}
          />
          {unit && (
            <button type="button" aria-label="Clear unit filter" onClick={() => setUnit('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              <X className="w-4 h-4" />
            </button>
          )}
        </div>

        <div className="relative flex-1 min-w-[220px] max-w-sm">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[var(--text-muted)]" />
          <input
            type="text"
            aria-label="Search log messages"
            placeholder="Search log messages (like --grep)"
            value={grep}
            onChange={e => setGrep(e.target.value)}
            className={`w-full pl-10 py-2 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)] ${grep ? 'pr-8' : 'pr-4'}`}
          />
          {grep && (
            <button type="button" aria-label="Clear message filter" onClick={() => setGrep('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-[var(--text-muted)] hover:text-[var(--text-primary)]">
              <X className="w-4 h-4" />
            </button>
          )}
        </div>

        <select
          aria-label="Boot"
          value={boot}
          onChange={e => setBoot(e.target.value)}
          className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        >
          {BOOT_FILTERS.map((b) => (
            <option key={b.label} value={b.value}>{b.label}</option>
          ))}
          {boots.map((b) => (
            <option key={`${b.index}-${b.boot_id}`} value={String(b.index)}>
              Boot {b.index} ({b.first_entry} .. {b.last_entry})
            </option>
          ))}
        </select>

        <input
          type="text"
          aria-label="Since"
          placeholder='Since (e.g. "1 hour ago" or 2026-04-20 10:00:00)'
          value={since}
          onChange={e => setSince(e.target.value)}
          className="min-w-[230px] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        />

        <input
          type="text"
          aria-label="Until"
          placeholder='Until (e.g. "now" or 2026-04-27 13:00:00)'
          value={until}
          onChange={e => setUntil(e.target.value)}
          className="min-w-[230px] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        />

        <input
          type="number"
          aria-label="UID filter"
          min={0}
          placeholder="UID"
          value={uid}
          onChange={e => setUid(e.target.value)}
          className="w-24 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        />

        <input
          type="number"
          aria-label="PID filter"
          min={0}
          placeholder="PID"
          value={pid}
          onChange={e => setPid(e.target.value)}
          className="w-28 bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        />

        <select
          aria-label="Line count"
          value={lineCount}
          onChange={e => setLineCount(Number(e.target.value))}
          className="bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2 text-sm focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
        >
          {LINE_COUNTS.map(n => (
            <option key={n} value={n}>{n} lines</option>
          ))}
        </select>

        <label className="flex items-center gap-2 text-sm text-[var(--text-muted)] cursor-pointer select-none">
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={e => setAutoRefresh(e.target.checked)}
            className="rounded border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] text-[var(--accent)] focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
          />
          Auto-refresh
        </label>

        <label className="flex items-center gap-2 text-sm text-[var(--text-muted)] cursor-pointer select-none">
          <input
            type="checkbox"
            checked={kernelOnly}
            onChange={e => setKernelOnly(e.target.checked)}
            className="rounded border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] text-[var(--accent)] focus-visible:ring-[color-mix(in_srgb,var(--machina-status-info)_50%,transparent)]"
          />
          Kernel only (-k)
        </label>
      </div>

      <div className="text-xs text-[var(--text-muted)]">
        Examples: <code>unit=sshd</code>, <code>priority=err</code>, <code>since=1 hour ago</code>, <code>boot=-1</code>, <code>uid=1000</code>, <code>pid=1234</code>, <code>grep=disconnect</code>
      </div>

      {/* Log entries */}
      {loading ? (
        <div className="flex items-center justify-center h-32" aria-busy="true" aria-label="Loading log entries">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-[var(--accent)]" />
        </div>
      ) : (
      <div className="bg-[var(--apple-surface)] rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 overflow-hidden">
        <div className="overflow-x-auto max-h-[70vh] overflow-y-auto font-mono text-xs">
          {entries.length === 0 ? (
            <div className="text-center text-[var(--text-muted)] py-12">No log entries found.</div>
          ) : (
            <table className="w-full" aria-label="System logs">
              <thead className="sticky top-0 bg-[var(--apple-fill-tertiary)] z-10">
                <tr className="border-b border-[var(--apple-hairline)] text-[var(--text-muted)] text-xs uppercase tracking-wider">
                  <th scope="col" className="text-left px-3 py-2 whitespace-nowrap">Timestamp</th>
                  <th scope="col" className="text-left px-3 py-2">Priority</th>
                  <th scope="col" className="text-left px-3 py-2">Unit</th>
                  <th scope="col" className="text-left px-3 py-2">Message</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((entry) => (
                  <tr key={`${entry.timestamp}-${entry.unit}`} className={`border-b border-[var(--apple-hairline)]/10 ${priorityBg(entry.priority)}`}>
                    <td className="px-3 py-1.5 text-[var(--text-muted)] whitespace-nowrap">{entry.timestamp}</td>
                    <td className={`px-3 py-1.5 font-semibold ${priorityColor(entry.priority)}`}>{entry.priority}</td>
                    <td className={`px-3 py-1.5 whitespace-nowrap ${statusToneClass('info')}`}>{entry.unit}</td>
                    <td className={`px-3 py-1.5 ${priorityColor(entry.priority)} break-all`}>{entry.message}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
      )}
    </PageLayout>
  )
}
