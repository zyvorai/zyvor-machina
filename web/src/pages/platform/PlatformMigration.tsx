// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { ArrowRightLeft, ExternalLink, Loader2 } from 'lucide-react'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import DetailTabs from '../../components/platform/DetailTabs'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import MigrationWavePlanner from '../../components/platform/MigrationWavePlanner'
import OpenStackImportPanel from '../../components/platform/OpenStackImportPanel'
import { getGuestkitStatus, guestkitDoctor, guestkitMigratePlan, submitGuestkitInspectJob, getGuestkitJob, listGuestkitJobsDaemon, getGuestkitCapabilitiesDaemon, type GuestkitJobRow } from '../../api/guestkit'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses } from '../../utils/semanticColors'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { tasksHubHref } from '../../utils/platformHubLinks'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'

const SOURCES = [
  { id: 'ova', label: 'OVF / OVA File', desc: 'Upload and convert' },
  { id: 'vmdk', label: 'VMDK File', desc: 'Single disk import' },
  { id: 'cloud', label: 'Cloud Image', desc: 'Ubuntu/RHEL cloud images' },
]

type MigrationTab = 'radar' | 'openstack' | 'waves' | 'jobs'

// Reused by several "enable this backend" call-to-action links on the page.
const INTEGRATIONS_ROUTE = '/platform/settings?section=integrations'

const MIGRATION_TABS = [
  { id: 'radar' as const, label: 'Import' },
  { id: 'openstack' as const, label: 'OpenStack' },
  { id: 'waves' as const, label: 'Plan waves' },
  { id: 'jobs' as const, label: 'GuestKit jobs' },
]

export default function PlatformMigration() {
  const { info } = usePlatformInfo()
  const [tier] = usePlatformDesktopTier()
  const navigate = useNavigate()
  const toast = useToastContext()
  const [tab, setTab] = usePlatformTabState<MigrationTab>(MIGRATION_TABS.map((t) => t.id), { defaultTab: 'radar' })
  const guestkit = Boolean(info?.guestkit?.enabled)
  const [gkStatus, setGkStatus] = useState<Awaited<ReturnType<typeof getGuestkitStatus>> | null>(null)
  const [diskPath, setDiskPath] = useState('')
  const [gkSummary, setGkSummary] = useState<string | null>(null)
  const [jobId, setJobId] = useState<string | null>(null)
  const [jobStatus, setJobStatus] = useState<string | null>(null)
  const [planSummary, setPlanSummary] = useState<string | null>(null)
  const [jobPolling, setJobPolling] = useState(false)
  const [gkJobs, setGkJobs] = useState<GuestkitJobRow[]>([])
  const [gkCaps, setGkCaps] = useState<string | null>(null)

  useEffect(() => {
    if (guestkit) {
      void getGuestkitStatus().then(setGkStatus).catch(() => {})
    }
  }, [guestkit])

  useEffect(() => {
    if (!jobId || !jobPolling) return
    const t = window.setInterval(() => {
      void getGuestkitJob(jobId).then((j) => {
        setJobStatus(j.summary ?? j.status)
        if (j.status === 'completed' || j.status === 'failed') setJobPolling(false)
      }).catch(() => setJobPolling(false))
    }, 2000)
    return () => window.clearInterval(t)
  }, [jobId, jobPolling])

  useEffect(() => {
    if (tab !== 'jobs' || !guestkit) return
    void listGuestkitJobsDaemon().then((rows) => setGkJobs(Array.isArray(rows) ? rows : [])).catch(() => setGkJobs([]))
    void getGuestkitCapabilitiesDaemon().then((c) => setGkCaps(c.summary ?? c.features?.join(', ') ?? null)).catch(() => setGkCaps(null))
  }, [tab, guestkit])

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Migration Radar"
      subtitle="Machina Migration Radar — single-VM import + GuestKit offline assurance."
      icon={<ArrowRightLeft className="w-6 h-6 text-[var(--text-muted)]" />}
      className="w-full max-w-none"
      contentClassName="space-y-4"
    >
      <DetailTabs primary={MIGRATION_TABS} active={tab} onChange={setTab} />

      {tab === 'jobs' && (
        <MacGlassPanel title="GuestKit job queue">
          <div className="flex flex-wrap gap-2 items-end mb-4">
            <label className="block flex-1 min-w-[14rem]">
              <span className="text-xs text-[var(--text-muted)]">Disk path</span>
              <input className="input text-sm mt-1 w-full" value={diskPath} onChange={(e) => setDiskPath(e.target.value)} />
            </label>
            <button
              type="button"
              className="btn-primary text-xs"
              disabled={!diskPath.trim() || !guestkit}
              onClick={async () => {
                try {
                  const r = await submitGuestkitInspectJob(diskPath.trim())
                  setJobId(r.job_id)
                  setJobStatus(r.summary)
                  setJobPolling(true)
                  toast.success(`Job ${r.job_id} submitted`)
                } catch (e: unknown) {
                  toast.error(formatUserError(e))
                }
              }}
            >
              Submit inspect job
            </button>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={!diskPath.trim() || !guestkit}
              onClick={async () => {
                try {
                  const r = await guestkitMigratePlan(diskPath.trim())
                  setPlanSummary(`${r.migration_score.toFixed(0)}% ready · ${r.summary}`)
                } catch (e: unknown) {
                  toast.error(formatUserError(e))
                }
              }}
            >
              Migrate plan
            </button>
          </div>
          {jobId && (
            <MacListRow
              title={`Job ${jobId}`}
              subtitle={jobStatus ?? 'Polling…'}
              badge={jobPolling ? <Loader2 className="w-4 h-4 animate-spin text-orange-400" /> : undefined}
            />
          )}
          {planSummary && <p className="text-xs text-[var(--text-muted)] mt-2">{planSummary}</p>}
          {gkCaps && <p className="text-xs text-orange-700/80 mt-3">Capabilities: {gkCaps}</p>}
          {gkJobs.length > 0 && (
            <ul className="mt-4 divide-y divide-white/[0.04]">
              {gkJobs.map((j) => (
                <MacListRow key={j.job_id} title={j.job_id} subtitle={`${j.status}${j.summary ? ` · ${j.summary}` : ''}`} />
              ))}
            </ul>
          )}
        </MacGlassPanel>
      )}

      {tab === 'waves' && (guestkit ? <MigrationWavePlanner /> : <p className="text-sm text-[var(--text-secondary)]">Wave planning needs GuestKit — enable it in Integrations.</p>)}

      {tab === 'openstack' && <OpenStackImportPanel />}

      {tab === 'radar' && (
      <>
      <div className="grid gap-3 sm:grid-cols-3">
        {guestkit && (
          <>
            <Link to="/platform/migration?tab=jobs" className="rounded-xl border border-[var(--apple-hairline)] bg-orange-500/10 p-4 text-sm hover:border-orange-400/50 transition">
              <p className="font-semibold text-orange-800">GuestKit jobs</p>
              <p className="text-xs text-orange-700/70 mt-1">Offline disk inspect and migrate planning.</p>
            </Link>
            <Link to="/platform/vms/v1?tab=guestHealth&guestAction=migrate-plan" className="rounded-xl border border-[var(--apple-hairline)] bg-orange-500/10 p-4 text-sm hover:border-orange-400/50 transition">
              <p className="font-semibold text-orange-800">Platform VM migrate plan</p>
              <p className="text-xs text-orange-700/70 mt-1">Run GuestKit offline KVM migration scoring on an enrolled VM disk.</p>
            </Link>
          </>
        )}
      </div>

      {guestkit && gkStatus && (
        <div className="rounded-xl border border-[var(--apple-hairline)] bg-orange-500/10 p-4 text-sm text-orange-800 space-y-2">
          <p>GuestKit {gkStatus.library_version ?? 'linked'} — {gkStatus.summary}</p>
          <div className="flex flex-wrap gap-2 items-end">
            <label className="block flex-1 min-w-[14rem]">
              <span className="text-xs text-orange-700/70">Offline disk path (qcow2/vmdk)</span>
              <input className="input text-sm mt-1 w-full" placeholder="/var/lib/libvirt/images/vm.qcow2" value={diskPath} onChange={(e) => setDiskPath(e.target.value)} />
            </label>
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={!diskPath.trim()}
              onClick={async () => {
                try {
                  const r = await guestkitDoctor(diskPath.trim(), 'kvm', true)
                  setGkSummary(`${r.boot_score.toFixed(0)}% boot · ${r.summary}`)
                } catch (e: unknown) {
                  setGkSummary(formatUserError(e))
                }
              }}
            >
              GuestKit doctor
            </button>
          </div>
          {gkSummary && <p className="text-xs text-orange-700/80">{gkSummary}</p>}
        </div>
      )}

      <section>
        <h2 className="text-sm font-semibold text-[var(--text-secondary)] mb-3">Where is your VM coming from?</h2>
        <div className="grid gap-3 sm:grid-cols-2">
          {SOURCES.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => navigate('/import')}
              className="text-left p-4 rounded-2xl border border-white/[0.08] bg-[var(--apple-surface)] hover:border-white/14 hover:bg-[var(--apple-surface)]/70 transition"
            >
              <p className="font-semibold text-[var(--text-primary)]">{s.label}</p>
              <p className="text-xs text-[var(--text-muted)] mt-1">{s.desc}</p>
            </button>
          ))}
        </div>
        {!guestkit && (
          <p className="mt-3 text-xs text-[var(--text-muted)]">
            GuestKit is not enabled. Use OVF/OVA or cloud image import, or enable GuestKit under{' '}
            <Link to={INTEGRATIONS_ROUTE} className={hubLinkClasses()}>Integrations</Link>.
          </p>
        )}
      </section>

      <section className="platform-mac-panel rounded-2xl border border-white/[0.06] p-5 space-y-4">
        <h2 className="font-semibold text-[var(--text-primary)]">Import</h2>
        <PlatformEmptyState
          icon={ArrowRightLeft}
          title="Bring a VM in"
          subtitle="Use single-VM import for OVF/OVA, VMDK and cloud images."
        >
          <Link to="/import" className="tahoe-btn-primary text-sm">Open import wizard</Link>
          <Link to={INTEGRATIONS_ROUTE} className={`tahoe-btn-ghost text-sm ${hubLinkClasses()}`}>Migration integrations</Link>
        </PlatformEmptyState>
        <p className="text-xs text-[var(--text-faint)] flex flex-wrap gap-3">
          <Link to="/import" className={`inline-flex items-center gap-1 ${hubLinkClasses()}`}>Single-VM import <ExternalLink className="w-3 h-3" /></Link>
          <Link to={INTEGRATIONS_ROUTE} className={hubLinkClasses()}>All migration tools →</Link>
          <Link to={tasksHubHref(tier)} className={hubLinkClasses()}>View migration tasks →</Link>
        </p>
      </section>
      </>
      )}
    </PlatformPageChrome>
  )
}
