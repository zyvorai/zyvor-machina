// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { ArrowLeft, Cloud, GitBranch, HardDrive, Network, RefreshCw, Server, Shield, CheckCircle2 } from 'lucide-react'
import ConfirmDialog from '../../../components/ConfirmDialog'
import {
  MacGlassPanel,
} from '../../../components/platform/mac/PlatformMacUi'
import { AppleDestinationList } from '../../../components/platform/apple/AppleStoryKit'
import JsonInspector, { asRecord, recordEntries } from '../../../components/platform/JsonInspector'
import PlatformFilterPills from '../../../components/platform/PlatformFilterPills'
import PageLayout from '../../../components/PageLayout'
import {
  getFirewallOverview,
  getBaremetalFirewallOverview,
  getMultisiteOverview,
  getMultisiteDrift,
  getMultisiteConnectivityMatrix,
  getMultisiteTimeline,
  getOperatorSecurePlan,
  getOperatorThresholds,
  executeOperatorSecure,
  executeOperatorSecureBatch,
  syncMultisiteFirewall,
  getZeusFirewallStatus,
  getFirewallScore,
  requestFirewallApproval,
  type FirewallOverview,
  type FirewallScore,
  type FirewallTargetSummary,
  type FleetSecurePlan,
  type MultisiteOverview,
} from '../../../api/zeusFirewall'
import { useToastContext } from '../../../contexts/ToastContext'
import { formatUserError } from '../../../utils/apiError'
import { hubLinkClasses, statusPillClasses, statusToneClass } from '../../../utils/semanticColors'
type KindFilter = 'all' | 'host' | 'bare_metal'

const FIREWALL_DESTINATIONS = [
  { to: '/platform/zeus/security/policies', title: 'Policy Studio', subtitle: 'Profile templates and rule authoring', icon: <Shield className="w-5 h-5" /> },
  { to: '/platform/zeus/security/ports', title: 'Open Ports', subtitle: 'Exposure scanner with process metadata', icon: <Network className="w-5 h-5" /> },
  { to: '/platform/zeus/security/services', title: 'Allowed Apps', subtitle: 'Permitted services and applications', icon: <Server className="w-5 h-5" /> },
  { to: '/platform/zeus/security/compliance', title: 'Compliance', subtitle: 'Grades, drift, and approval workflows', icon: <CheckCircle2 className="w-5 h-5" /> },
  { to: '/platform/zeus/security/activity', title: 'Activity', subtitle: 'Blocked and allowed connections', icon: <Shield className="w-5 h-5" /> },
  { to: '/platform/zeus/security/connectivity', title: 'Connectivity', subtitle: 'Cross-site and inter-host reachability', icon: <Network className="w-5 h-5" /> },
  { to: '/platform/zeus/security/cloud', title: 'Cloud SGs', subtitle: 'Security group alignment', icon: <Cloud className="w-5 h-5" /> },
  { to: '/platform/zeus/security/k8s', title: 'Kubernetes', subtitle: 'Cluster network policies', icon: <GitBranch className="w-5 h-5" /> },
]

export default function PlatformFirewallOverview() {
  const toast = useToastContext()
  const [overview, setOverview] = useState<FirewallOverview | null>(null)
  const [multisite, setMultisite] = useState<MultisiteOverview | null>(null)
  const [operatorPlan, setOperatorPlan] = useState<FleetSecurePlan | null>(null)
  const [statusLine, setStatusLine] = useState<string | null>(null)
  const [kindFilter, setKindFilter] = useState<KindFilter>('all')
  const [error, setError] = useState<string | null>(null)
  const [operatorBusy, setOperatorBusy] = useState(false)
  const [syncBusy, setSyncBusy] = useState(false)
  const [applyProfiles, setApplyProfiles] = useState(true)
  const [includeLockdown, setIncludeLockdown] = useState(false)
  const [multisiteTab, setMultisiteTab] = useState<'overview' | 'drift' | 'connectivity' | 'timeline'>('overview')
  const [multisiteExtra, setMultisiteExtra] = useState<Record<string, unknown> | Array<Record<string, unknown>> | null>(null)
  const [thresholds, setThresholds] = useState<Record<string, unknown> | null>(null)
  const [baremetalFw, setBaremetalFw] = useState<Record<string, unknown> | null>(null)
  const [scoreBusy, setScoreBusy] = useState(false)
  const [scoreSample, setScoreSample] = useState<FirewallScore | null>(null)
  const [approvalBusy, setApprovalBusy] = useState(false)
  const [confirmApplyBatch, setConfirmApplyBatch] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [ov, st, ms, op, th, bm] = await Promise.all([
        getFirewallOverview(),
        getZeusFirewallStatus(),
        getMultisiteOverview().catch(() => null),
        getOperatorSecurePlan().catch(() => null),
        getOperatorThresholds().catch(() => null),
        getBaremetalFirewallOverview().catch(() => null),
      ])
      setOverview(ov)
      setMultisite(ms)
      setOperatorPlan(op)
      setThresholds(th)
      setBaremetalFw(bm as Record<string, unknown> | null)
      const bpf = st.native_bpf as { summary?: string }
      setStatusLine(bpf?.summary || 'Zeus Firewall active')
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  const targets = useMemo(
    () => (overview && !Array.isArray(overview) && Array.isArray(overview.targets) ? overview.targets : []),
    [overview],
  )

  const filtered = useMemo(() => {
    if (kindFilter === 'all') return targets
    return targets.filter((t) => t.kind === kindFilter)
  }, [kindFilter, targets])

  const metalCount = targets.filter((t) => t.kind === 'bare_metal').length
  const hostCount = targets.filter((t) => t.kind === 'host').length
  const scoreTarget = filtered.find((t) => t.risk === 'critical' || t.risk === 'high') ?? filtered[0]

  const loadScoreSample = async () => {
    if (!scoreTarget) return
    setScoreBusy(true)
    try {
      setScoreSample(await getFirewallScore(scoreTarget.id))
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setScoreBusy(false)
    }
  }

  const runRequestApproval = async () => {
    if (!scoreTarget) return
    setApprovalBusy(true)
    try {
      await requestFirewallApproval({
        target_id: scoreTarget.id,
        profile: scoreTarget.profile ?? undefined,
      })
      toast.success('Firewall change approval requested')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setApprovalBusy(false)
    }
  }

  const runOperatorSecure = async (hostId: string, profile?: string, dryRun = true) => {
    setOperatorBusy(true)
    try {
      const r = await executeOperatorSecure({ host_id: hostId, profile, dry_run: dryRun })
      toast.success(r.message || (dryRun ? 'Dry-run complete' : 'Operator secure applied'))
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setOperatorBusy(false)
    }
  }

  const runOperatorBatch = async (dryRun: boolean) => {
    setOperatorBusy(true)
    try {
      const r = await executeOperatorSecureBatch({ dry_run: dryRun, auto_only: true })
      toast.success(r.summary)
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setOperatorBusy(false)
    }
  }

  const runMultisiteSync = async () => {
    setSyncBusy(true)
    try {
      const r = await syncMultisiteFirewall({
        source_site: 'primary-local',
        target_site: 'dr-replica',
        apply_profiles: applyProfiles,
        include_lockdown: includeLockdown,
      })
      toast.success(r.summary)
      if ((r.apply_errors ?? []).length > 0) {
        toast.warning(`${r.apply_errors.length} apply error(s) — check agent connectivity`)
      }
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setSyncBusy(false)
    }
  }

  return (
    <PageLayout
      compact
      error={error}
      prepend={
        <Link to="/platform/zeus/security" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Security Center
        </Link>
      }
      title="Zeus Firewall"
      subtitle={
        <span className="flex flex-wrap items-center gap-2 text-sm">
          {overview && !Array.isArray(overview) && (
            <>
              <span className={statusPillClasses(overview.critical_count > 0 ? 'error' : 'ok')}>
                {overview.critical_count} critical
              </span>
              <span className="text-[var(--text-muted)]">
                {targets.length} machines · {targets.filter((t) => t.risk === 'low').length} compliant
              </span>
            </>
          )}
          {statusLine && <span className="text-[var(--text-muted)]">{statusLine}</span>}
        </span>
      }
      icon={<Shield className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <button type="button" className="btn-secondary text-xs" onClick={() => void load()} aria-label="Refresh">
          <RefreshCw className="w-4 h-4" />
        </button>
      }
      contentClassName="space-y-4"
    >
      {overview && targets.length > 0 && (
        <>
          <PlatformFilterPills
            value={kindFilter}
            onChange={(id) => setKindFilter(id as KindFilter)}
            options={[
              { id: 'all', label: 'All', count: targets.length },
              { id: 'host', label: 'Hosts', count: hostCount },
              { id: 'bare_metal', label: 'Bare metal', count: metalCount },
            ]}
          />
          <AppleDestinationList items={FIREWALL_DESTINATIONS} />
          {scoreTarget && (
            <MacGlassPanel title="Risk scoring & approvals" subtitle={`Sample target: ${scoreTarget.name} · POST /targets/{id}/score and /approvals`}>
              <div className="flex flex-wrap gap-2 mb-3">
                <button type="button" className="btn-secondary text-xs" disabled={scoreBusy} onClick={() => void loadScoreSample()}>
                  {scoreBusy ? 'Scoring…' : 'Score sample target'}
                </button>
                <button type="button" className="btn-primary text-xs" disabled={approvalBusy} onClick={() => void runRequestApproval()}>
                  {approvalBusy ? 'Requesting…' : 'Request approval'}
                </button>
              </div>
              {scoreSample && (
                <div className="text-sm text-[var(--text-secondary)] space-y-2">
                  <p>Score: <span className="font-semibold text-[var(--text-primary)]">{scoreSample.score}</span></p>
                  {(scoreSample.breakdown ?? []).slice(0, 3).map((b) => (
                    <p key={b.category} className="text-xs text-[var(--text-muted)]">{b.category}: {b.detail} ({b.points} pts)</p>
                  ))}
                </div>
              )}
            </MacGlassPanel>
          )}
          <MacGlassPanel title="Machines" subtitle={`${overview.summary} · ${filtered.length} shown`}>
            <AppleDestinationList
              items={filtered.map((t: FirewallTargetSummary) => ({
                to: `/platform/zeus/security/firewall/${t.id}`,
                title: t.name,
                subtitle: [
                  t.kind === 'bare_metal' ? 'Bare metal' : 'Host',
                  `${t.risk} risk`,
                  `${t.open_ports} open port${t.open_ports === 1 ? '' : 's'}`,
                  t.profile || null,
                ]
                  .filter(Boolean)
                  .join(' · '),
                icon: t.kind === 'bare_metal' ? <HardDrive className="w-5 h-5" /> : <Shield className="w-5 h-5" />,
              }))}
            />
          </MacGlassPanel>
          {operatorPlan && (operatorPlan.previews?.length ?? 0) > 0 && (
            <MacGlassPanel title="AI operator" subtitle={operatorPlan.summary}>
              <div className="flex flex-wrap gap-2 mb-3">
                <button type="button" className="btn-secondary text-xs" disabled={operatorBusy} onClick={() => void runOperatorBatch(true)}>
                  Dry-run auto-eligible
                </button>
                <button type="button" className="btn-primary text-xs" disabled={operatorBusy || operatorPlan.auto_eligible === 0} onClick={() => setConfirmApplyBatch(true)}>
                  Apply auto-eligible ({operatorPlan.auto_eligible})
                </button>
              </div>
              <ul className="text-xs text-[var(--text-muted)] space-y-1 max-h-32 overflow-y-auto">
                {operatorPlan.previews.slice(0, 6).map((p) => (
                  <li key={p.host_id} className="flex flex-wrap items-center gap-2">
                    <span>
                      {p.hostname} → {p.target_profile}
                      {p.requires_approval ? ' · needs approval' : ' · auto-eligible'}
                    </span>
                    <button
                      type="button"
                      className="btn-secondary text-xs py-0.5 px-1.5"
                      disabled={operatorBusy}
                      data-testid={`operator-secure-${p.host_id}`}
                      onClick={() => void runOperatorSecure(p.host_id, p.target_profile, true)}
                    >
                      Dry-run host
                    </button>
                  </li>
                ))}
              </ul>
            </MacGlassPanel>
          )}
          {multisite && (
            <MacGlassPanel title="Multi-site federation" subtitle={multisite.summary}>
              <div className="flex flex-wrap gap-2 mb-3">
                {(['overview', 'drift', 'connectivity', 'timeline'] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    className={`text-xs px-3 py-1 rounded-full ${multisiteTab === t ? 'bg-[var(--accent)] text-white' : 'bg-white/10 text-[var(--text-secondary)]'}`}
                    onClick={() => {
                      setMultisiteTab(t)
                      if (t === 'drift') void getMultisiteDrift().then(setMultisiteExtra).catch(() => setMultisiteExtra(null))
                      if (t === 'connectivity') void getMultisiteConnectivityMatrix().then(setMultisiteExtra).catch(() => setMultisiteExtra(null))
                      if (t === 'timeline') void getMultisiteTimeline().then(setMultisiteExtra).catch(() => setMultisiteExtra(null))
                    }}
                  >
                    {t}
                  </button>
                ))}
              </div>
              {multisiteTab === 'overview' && (
              <>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 -mt-1">
                {multisite.sites.map((s) => (
                  <div key={s.id} className="rounded-xl border border-white/[0.06] bg-[var(--apple-surface)] px-3 py-2">
                    <p className="text-sm text-[var(--text-primary)]">{s.name} <span className="text-[var(--text-muted)]">({s.role})</span></p>
                    <p className="text-xs text-[var(--text-muted)] mt-0.5">{s.gitops_namespace} · {s.target_count} targets · grade {(multisite.compliance_rollup?.sites ?? []).find((c) => c.site === s.name)?.grade ?? '—'}</p>
                  </div>
                ))}
              </div>
              <div className="mt-3 flex flex-wrap items-center gap-3 text-xs text-[var(--text-muted)]">
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={applyProfiles} onChange={(e) => setApplyProfiles(e.target.checked)} />
                  Apply synced profiles to online hosts
                </label>
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={includeLockdown} onChange={(e) => setIncludeLockdown(e.target.checked)} />
                  DR lockdown profile
                </label>
                <button type="button" className="btn-secondary text-xs" disabled={syncBusy} onClick={() => void runMultisiteSync()}>
                  {syncBusy ? 'Syncing…' : 'Sync primary → DR'}
                </button>
              </div>
              {(multisite.policy_conflicts?.length ?? 0) > 0 && (
                <ul className={`mt-3 text-xs space-y-1 ${statusToneClass('warn')}`}>
                  {multisite.policy_conflicts.map((c) => (
                    <li key={c.id}>{c.policy_name}: {c.detail}</li>
                  ))}
                </ul>
              )}
              </>
              )}
              {multisiteTab !== 'overview' && multisiteExtra && (
                <JsonInspector data={multisiteExtra} className="mt-3" />
              )}
            </MacGlassPanel>
          )}
          {thresholds && (
            <MacGlassPanel title="Operator thresholds" subtitle={String(thresholds.summary ?? 'Auto-secure eligibility rules')}>
              <div className="apple-metric-band -mt-1">
                {recordEntries(asRecord(thresholds) ?? {}, 8).map(([k, v]) => (
                  <div key={k} className="min-w-0">
                    <div className="apple-metric-value">{String(v)}</div>
                    <div className="apple-metric-label">{k.replace(/_/g, ' ')}</div>
                  </div>
                ))}
              </div>
              <JsonInspector data={thresholds} className="mt-3" />
            </MacGlassPanel>
          )}
          {baremetalFw && (
            <MacGlassPanel title="Bare-metal firewall rollup" subtitle="BMC-attached servers under Zeus Firewall">
              <JsonInspector data={baremetalFw} />
            </MacGlassPanel>
          )}
        </>
      )}
      <ConfirmDialog
        open={confirmApplyBatch}
        title="Apply Firewall Changes"
        message={`Apply auto-secure firewall profiles to ${operatorPlan?.auto_eligible ?? 0} auto-eligible host(s) now? This changes live firewall rules on those machines.`}
        confirmLabel="Apply"
        variant="danger"
        onCancel={() => setConfirmApplyBatch(false)}
        onConfirm={() => {
          setConfirmApplyBatch(false)
          void runOperatorBatch(false)
        }}
      />
    </PageLayout>
  )
}
