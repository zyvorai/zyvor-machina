// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, useCallback, useMemo, useRef } from 'react'
import { Link, useParams } from 'react-router'
import { Activity, ChevronLeft, RefreshCw } from 'lucide-react'
import { BuildStepTimeline } from '../components/BuildStepTimeline'
import TerminalFrame from '../components/TerminalFrame'
import { renderHighlightedLog } from '../utils/terminalHighlight'
import { getJob, listJobs, JobDetail, JobSummary } from '../api/jobs'
import {
  computePackerJobTimeline,
  computeVirtImageBuildTimeline,
  computeVmCreateJobTimeline,
  GOLDEN_FORGE_TIMELINE_LABELS,
  VIRT_IMAGE_TIMELINE_LABELS,
  VM_CREATE_TIMELINE_LABELS,
} from '../utils/buildProgress'
import { useToastContext } from '../contexts/ToastContext'
import { useBreadcrumbName } from '../contexts/BreadcrumbNameContext'
import { formatUserError } from '../utils/apiError'
import EmptyState from '../components/EmptyState'
import PageLayout from '../components/PageLayout'
import { jobStatusTone, statusSurfaceClasses, statusToneClass } from '../utils/semanticColors'

function statusBadge(status: string) {
  return statusSurfaceClasses(jobStatusTone(status), 'px-2 py-0.5 rounded text-xs font-medium border')
}

export default function JobsPage() {
  const { jobId } = useParams<{ jobId?: string }>()
  const toast = useToastContext()
  const [jobs, setJobs] = useState<JobSummary[]>([])
  const [detail, setDetail] = useState<JobDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)

  const selectedId = jobId || null

  useBreadcrumbName(detail?.title ?? '')

  const jobTimeline = useMemo(() => {
    if (!detail) return null
    if (detail.kind === 'packer_golden_build') {
      return {
        steps: GOLDEN_FORGE_TIMELINE_LABELS,
        variant: 'violet' as const,
        ...computePackerJobTimeline(detail.logs, detail.status),
      }
    }
    if (detail.kind === 'vm_create') {
      return {
        steps: VM_CREATE_TIMELINE_LABELS,
        variant: 'amber' as const,
        ...computeVmCreateJobTimeline(detail.logs, detail.status),
      }
    }
    return {
      steps: VIRT_IMAGE_TIMELINE_LABELS,
      variant: 'slate' as const,
      ...computeVirtImageBuildTimeline(detail.logs, detail.status),
    }
  }, [detail])

  const refreshList = useCallback(() => {
    return listJobs()
      .then((data) => {
        setJobs(data)
        setLoadError(null)
      })
      .catch((e: unknown) => {
        const msg = formatUserError(e)
        setLoadError(msg)
        toast.error(msg)
      })
  }, [toast])

  // Monotonic request id so an out-of-order response for a previously-selected
  // job can't overwrite the currently-selected job's detail pane.
  const detailReqRef = useRef(0)
  const refreshDetail = useCallback(() => {
    if (!selectedId) {
      setDetail(null)
      return
    }
    const reqId = ++detailReqRef.current
    getJob(selectedId)
      .then((d) => { if (detailReqRef.current === reqId) setDetail(d) })
      .catch((e: unknown) => { if (detailReqRef.current === reqId) toast.error(formatUserError(e)) })
  }, [selectedId, toast])

  useEffect(() => {
    setLoading(true)
    void refreshList().finally(() => setLoading(false))
  }, [refreshList])

  useEffect(() => {
    const t = window.setInterval(refreshList, 5000)
    return () => window.clearInterval(t)
  }, [refreshList])

  useEffect(() => {
    // Clear the previous job's detail immediately on switch so its logs/status
    // don't linger under the new selection while the fetch is in flight.
    setDetail(null)
    refreshDetail()
  }, [refreshDetail])

  useEffect(() => {
    if (!selectedId || !detail || detail.status !== 'running') return
    const t = window.setInterval(refreshDetail, 1200)
    return () => window.clearInterval(t)
  }, [selectedId, detail?.status, refreshDetail])

  return (
    <PageLayout
      eyebrow="Hypervisor"
      className="mx-auto max-w-6xl px-4"
      loading={loading && jobs.length === 0}
      title="Jobs"
      icon={<Activity className={`w-7 h-7 ${statusToneClass('warn')}`} />}
      subtitle={
        <>
          Monitor <strong className="text-[var(--text-secondary)]">virt-image-build</strong>,{' '}
          <strong className="text-[var(--text-secondary)]">Golden Forge</strong> (Packer / dockur qcow2), and{' '}
          <strong className="text-[var(--text-secondary)]">Create VM</strong> progress after you navigate away. Logs update automatically while a job is running.
        </>
      }
      actions={
        <>
          <Link to="/" className="p-2 hover:bg-[var(--surface-hover)] rounded transition" aria-label="Dashboard" title="Back to dashboard">
            <ChevronLeft className="w-5 h-5" />
          </Link>
          <button
            type="button"
            onClick={() => {
              refreshList()
              refreshDetail()
            }}
            className="btn-secondary text-sm inline-flex items-center gap-1.5"
          >
            <RefreshCw className="w-4 h-4" aria-hidden />
            Refresh
          </button>
        </>
      }
      error={loadError}
      errorTitle="Could not load jobs"
      errorHints={['Confirm machina-daemon is running.', 'Jobs require a valid session with operator or admin role.']}
      onErrorRetry={() => void refreshList()}
    >
      <div className="flex flex-col gap-6 w-full">
        <div className="tahoe-glass-card p-4">
          <h2 className="text-lg font-semibold text-[var(--text-primary)] mb-3">Recent jobs</h2>
          {jobs.length === 0 ? (
            <EmptyState
              icon={<Activity className="w-6 h-6" />}
              title="No jobs yet"
              description="Start a disk build, Golden Forge qcow2 build, or create a VM with streaming logs — progress appears here after you navigate away."
              primaryAction={
                <Link to="/disk-images" className="btn-primary text-sm">
                  Disk images
                </Link>
              }
              secondaryAction={
                <Link to="/create" className="px-4 py-2 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] text-sm text-[var(--text-primary)] border border-[var(--apple-hairline)]">
                  Create VM
                </Link>
              }
            />
          ) : (
            <ul className="space-y-2 max-h-[32rem] overflow-y-auto divide-y divide-[var(--apple-hairline)]/80">
              {jobs.map((j) => (
                <li key={j.id} className="pt-2 first:pt-0">
                  <Link
                    to={`/jobs/${encodeURIComponent(j.id)}`}
                    className={`block rounded-lg px-3 py-2 transition hover:bg-[var(--apple-fill-tertiary)]/80 ${
                      selectedId === j.id ? 'bg-[var(--apple-fill-tertiary)] ring-1 ring-amber-500/40' : ''
                    }`}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-mono text-xs text-[var(--text-muted)] truncate">{j.id}</span>
                      <span className={`text-[10px] uppercase tracking-wide px-2 py-0.5 rounded ${statusBadge(j.status)}`}>
                        {j.status}
                      </span>
                    </div>
                    <div className="text-sm text-[var(--text-primary)] mt-1">{j.title}</div>
                    <div className="text-[10px] text-[var(--text-muted)] mt-0.5">
                      {j.kind.replace(/_/g, ' ')}
                      {j.vm_name ? ` · VM ${j.vm_name}` : ''}
                      {j.target_path ? ` · ${j.target_path}` : ''}
                    </div>
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="tahoe-glass-card p-4 flex flex-col min-h-[20rem]">
          {!selectedId ? (
            <p className="text-[var(--text-muted)] text-sm">Select a job from the list to view logs.</p>
          ) : !detail ? (
            <p className="text-[var(--text-muted)] text-sm">Loading job…</p>
          ) : (
            <>
              <div className="flex flex-wrap items-start justify-between gap-2 mb-3">
                <div>
                  <h2 className="text-lg font-semibold text-[var(--text-primary)]">Job detail</h2>
                  <p className="font-mono text-xs text-[var(--text-muted)] break-all mt-1">{detail.id}</p>
                </div>
                <span className={`text-[10px] uppercase tracking-wide px-2 py-1 rounded ${statusBadge(detail.status)}`}>
                  {detail.status}
                </span>
              </div>
              <p className="text-sm text-[var(--text-secondary)] mb-2">{detail.title}</p>
              {detail.error ? (
                <p className="text-sm text-rose-600 mb-2">{detail.error}</p>
              ) : null}
              {jobTimeline ? (
                <BuildStepTimeline
                  className="mb-3"
                  steps={jobTimeline.steps}
                  activeIndex={jobTimeline.activeIndex}
                  allComplete={jobTimeline.allComplete}
                  failed={jobTimeline.failed}
                  variant={jobTimeline.variant}
                />
              ) : null}
              <div className="flex-1 min-h-0 flex flex-col">
                <h3 className="text-xs font-medium text-[var(--text-muted)] uppercase tracking-wide mb-1">Log</h3>
                <TerminalFrame label="Job log" className="flex-1 min-h-0 flex flex-col" maxHeight="flex-1 min-h-0 max-h-[50vh]">
                  {detail.logs.length ? renderHighlightedLog(detail.logs.join('\n')) : '(no log lines yet)'}
                </TerminalFrame>
              </div>
            </>
          )}
        </div>
      </div>
    </PageLayout>
  )
}
