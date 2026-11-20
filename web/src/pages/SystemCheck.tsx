// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import {
  Activity,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  AlertTriangle,
  XCircle,
  MinusCircle,
  RefreshCw,
  Copy,
  Stethoscope,
} from 'lucide-react'
import { useWebSocketContext } from '../contexts/WebSocketContext'
import { useToastContext } from '../contexts/ToastContext'
import ErrorBanner from '../components/ErrorBanner'
import PageLayout from '../components/PageLayout'
import { libvirtErrorHints } from '../utils/libvirtHints'
import { formatUserError } from '../utils/apiError'
import {
  CHECK_CATEGORY_LABELS,
  CHECK_CATEGORY_ORDER,
  type CheckCategory,
  type CheckResult,
  type CheckStatus,
  formatCheckReportMarkdown,
  resultsByCategory,
  runSystemCheckSuite,
  summarizeCheckResults,
} from '../lib/systemCheckSuite'
import { checkStatusTone, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

function statusIcon(status: CheckStatus) {
  switch (status) {
    case 'pass':
      return <CheckCircle2 className={`w-4 h-4 shrink-0 ${statusToneClass('ok')}`} />
    case 'warn':
      return <AlertTriangle className={`w-4 h-4 shrink-0 ${statusToneClass('warn')}`} />
    case 'fail':
      return <XCircle className={`w-4 h-4 shrink-0 ${statusToneClass('error')}`} />
    case 'skip':
      return <MinusCircle className="w-4 h-4 text-[var(--text-muted)] shrink-0" />
  }
}

function overallBadgeClass(overall: 'pass' | 'warn' | 'fail') {
  const tone = checkStatusTone(overall)
  return statusSurfaceClasses(tone, 'px-4 py-4 rounded-xl border')
}

function rowClass(status: CheckStatus) {
  if (status === 'skip') return 'opacity-60'
  return ''
}

function categoryHints(category: CheckCategory, failedMessages: string): string[] {
  const msg = failedMessages.toLowerCase()
  if (category === 'host' || category === 'libvirt' || category === 'services') {
    return libvirtErrorHints(msg)
  }
  return []
}

function CheckRow({ result }: { result: CheckResult }) {
  const [open, setOpen] = useState(false)
  const hasDetail = result.detail !== undefined && result.detail !== null

  return (
    <div className={`border-b border-[var(--apple-hairline)]/40 last:border-0 ${rowClass(result.status)}`}>
      <button
        type="button"
        className="w-full flex items-start gap-3 px-4 py-3 text-left hover:bg-[var(--apple-surface)] transition"
        onClick={() => hasDetail && setOpen((o) => !o)}
        disabled={!hasDetail}
      >
        {statusIcon(result.status)}
        <div className="flex-1 min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-medium text-[var(--text-primary)] text-sm">{result.label}</span>
            <span className="text-xs text-[var(--text-muted)] font-mono uppercase">{result.status}</span>
            {result.durationMs > 0 && (
              <span className="text-xs text-[var(--text-faint)]">{result.durationMs}ms</span>
            )}
          </div>
          <p className="text-sm text-[var(--text-muted)] mt-0.5">{result.message}</p>
        </div>
        {hasDetail && (
          <span className="text-[var(--text-muted)] mt-0.5">
            {open ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
          </span>
        )}
      </button>
      {open && hasDetail && (
        <pre className="mx-4 mb-3 p-3 rounded-lg bg-[var(--apple-surface)]/80 border border-[var(--apple-hairline)] text-xs text-[var(--text-muted)] overflow-x-auto max-h-48">
          {JSON.stringify(result.detail, null, 2)}
        </pre>
      )}
    </div>
  )
}

function CategorySection({
  category,
  rows,
  defaultOpen,
}: {
  category: CheckCategory
  rows: CheckResult[]
  defaultOpen?: boolean
}) {
  const [expanded, setExpanded] = useState(defaultOpen ?? true)
  const failed = rows.filter((r) => r.status === 'fail')
  const warns = rows.filter((r) => r.status === 'warn')
  const catOverall: 'pass' | 'warn' | 'fail' =
    failed.length > 0 ? 'fail' : warns.length > 0 ? 'warn' : 'pass'
  const hints = categoryHints(
    category,
    [...failed, ...warns].map((r) => r.message).join(' '),
  )

  return (
    <section className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/80 bg-[var(--apple-surface)] overflow-hidden">
      <button
        type="button"
        className="w-full flex items-center justify-between gap-3 px-4 py-3 hover:bg-[var(--apple-fill-tertiary)]/30 transition text-left"
        onClick={() => setExpanded((e) => !e)}
      >
        <div className="flex items-center gap-2">
          {expanded ? (
            <ChevronDown className="w-4 h-4 text-[var(--text-muted)]" />
          ) : (
            <ChevronRight className="w-4 h-4 text-[var(--text-muted)]" />
          )}
          <h2 className="font-semibold text-[var(--text-primary)]">{CHECK_CATEGORY_LABELS[category]}</h2>
          <span
            className={`text-xs px-2 py-0.5 rounded-full border ${overallBadgeClass(catOverall)}`}
          >
            {rows.filter((r) => r.status === 'pass').length}/{rows.length} OK
          </span>
        </div>
      </button>
      {expanded && (
        <>
          {failed.length > 0 && hints.length > 0 && (
            <div className="px-4 pb-2">
              <ErrorBanner
                title={`${CHECK_CATEGORY_LABELS[category]} issues`}
                headline={failed.map((r) => r.message).join(' · ')}
                hints={hints}
                tone="red"
              />
            </div>
          )}
          <div>
            {rows.map((r) => (
              <CheckRow key={r.id} result={r} />
            ))}
          </div>
        </>
      )}
    </section>
  )
}

export default function SystemCheckPage() {
  const { isConnected: wsConnected } = useWebSocketContext()
  const toast = useToastContext()

  const [results, setResults] = useState<CheckResult[]>([])
  const [running, setRunning] = useState(false)
  const [progressLabel, setProgressLabel] = useState<string | null>(null)

  const summary = useMemo(() => summarizeCheckResults(results), [results])
  const byCategory = useMemo(() => resultsByCategory(results), [results])

  const runChecks = useCallback(async () => {
    setRunning(true)
    setProgressLabel('Starting…')
    try {
      const { results: r } = await runSystemCheckSuite(
        { wsConnected },
        (p) => setProgressLabel(p.label),
      )
      setResults(r)
    } catch (e: unknown) {
      toast.error(`System check failed: ${formatUserError(e)}`)
    } finally {
      setRunning(false)
      setProgressLabel(null)
    }
  }, [wsConnected, toast])

  useEffect(() => {
    void runChecks()
  }, [runChecks])

  const copyReport = async () => {
    const md = formatCheckReportMarkdown(results, summary)
    try {
      await navigator.clipboard.writeText(md)
      toast.success('Report copied to clipboard')
    } catch {
      toast.error('Copy failed')
    }
  }

  const busy = running

  return (
    <PageLayout
      eyebrow="System"
      className="max-w-4xl"
      title="System Check"
      icon={<Stethoscope className="w-7 h-7 text-[var(--link)]" />}
      subtitle={
        <>
          Auto-runs read-only diagnostics (API, host, libvirt, Fleet Cloud, Kubernetes, services).
          Same coverage as <code className="text-[var(--text-muted)]">e2e-test.sh</code> preflight — from the UI.
        </>
      }
      actions={
        <>
          <button
            type="button"
            disabled={busy}
            onClick={() => void runChecks()}
            className="btn-primary text-sm inline-flex items-center gap-2 disabled:opacity-50"
          >
            <RefreshCw className={`w-4 h-4 ${running ? 'animate-spin' : ''}`} />
            Run again
          </button>
          <button
            type="button"
            disabled={results.length === 0 || busy}
            onClick={() => void copyReport()}
            className="btn-secondary text-sm inline-flex items-center gap-2 disabled:opacity-50"
          >
            <Copy className="w-4 h-4" />
            Copy report
          </button>
        </>
      }
      contentClassName="space-y-6"
    >
      {running && progressLabel && (
        <div className="flex items-center gap-3 px-4 py-3 rounded-xl border border-[var(--accent)]/40 bg-[var(--accent-soft)] text-sm text-[var(--link)]">
          <RefreshCw className="w-4 h-4 animate-spin shrink-0" />
          {progressLabel}
        </div>
      )}

      {results.length > 0 && !running && (
        <div
          className={`flex flex-wrap items-center gap-4 px-4 py-4 rounded-xl border ${overallBadgeClass(summary.overall)}`}
        >
          <span className="text-lg font-semibold capitalize">{summary.overall}</span>
          <span className="text-sm">
            <span className={statusToneClass('ok')}>{summary.pass} passed</span>
            {' · '}
            <span className={statusToneClass('warn')}>{summary.warn} warnings</span>
            {' · '}
            <span className={statusToneClass('error')}>{summary.fail} failed</span>
            {summary.skip > 0 && (
              <>
                {' · '}
                <span className="text-[var(--text-muted)]">{summary.skip} skipped</span>
              </>
            )}
          </span>
        </div>
      )}

      <div className="flex flex-wrap gap-2 text-xs">
        <Link to="/services" className="text-[var(--link)] hover:underline">
          Services
        </Link>
        <span className="text-[var(--text-faint)]">·</span>
        <Link to="/capabilities" className="text-[var(--link)] hover:underline">
          Capabilities
        </Link>
        <span className="text-[var(--text-faint)]">·</span>
        <Link to="/node" className="text-[var(--link)] hover:underline">
          Host overview
        </Link>
      </div>

      {running && results.length === 0 && (
        <div className="flex items-center justify-center py-16 text-[var(--text-muted)]">
          <RefreshCw className="w-8 h-8 animate-spin text-[var(--link)] mr-3" />
          Running system check…
        </div>
      )}

      <div className="space-y-4">
        {CHECK_CATEGORY_ORDER.map((cat) => {
          const rows = byCategory.get(cat)
          if (!rows?.length) return null
          return (
            <CategorySection
              key={cat}
              category={cat}
              rows={rows}
              defaultOpen={rows.some((r) => r.status === 'fail' || r.status === 'warn')}
            />
          )
        })}
      </div>

      {results.length > 0 && summary.fail === 0 && summary.warn === 0 && !running && (
        <div className={`flex items-center gap-2 text-sm px-4 py-3 rounded-xl border ${statusSurfaceClasses('ok')}`}>
          <Activity className="w-4 h-4" />
          All checks passed.
        </div>
      )}

    </PageLayout>
  )
}
