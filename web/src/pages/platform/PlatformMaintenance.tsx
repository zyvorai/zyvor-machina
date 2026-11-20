// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link, useSearchParams } from 'react-router'
import {
  CalendarClock,
  Download,
  ListChecks,
  Loader2,
  Plus,
  Trash2,
} from 'lucide-react'
import {
  MacGlassPanel,
  MacListRow,
} from '../../components/platform/mac/PlatformMacUi'
import DetailTabs from '../../components/platform/DetailTabs'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton } from '../../components/platform/PlatformPageChrome'
import { usePlatformTabState } from '../../hooks/usePlatformTabState'
import ErrorBanner from '../../components/ErrorBanner'
import PageSkeleton from '../../components/PageSkeleton'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import HostEnrollWizard from '../../components/platform/HostEnrollWizard'
import { BuildStepTimeline } from '../../components/BuildStepTimeline'
import {
  createMaintenanceSchedule,
  deleteMaintenanceSchedule,
  getFleetMaintenanceMission,
  getFleetUpdates,
  getUpgradeMatrix,
  type UpgradeHostSkew,
  type UpgradeMatrix,
  hostMaintenance,
  listMaintenanceSchedules,
  listPlatformHosts,
  upgradeHostAgent,
  previewHostPackageUpgrade,
  applyHostPackageUpgrade,
  type FleetMaintenanceMissionOverview,
  type FleetUpdatesOverview,
  type MaintenanceMissionHost,
  type MaintenanceSchedule,
  type MaintenanceStepStatus,
  type PlatformHost,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusChipClasses, statusSurfaceClasses } from '../../utils/semanticColors'
import { operationsHubHref } from '../../utils/platformHubLinks'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { toastQueuedOperation } from '../../utils/platformTaskToast'

type TabId = 'mission' | 'updates' | 'schedules'

const MAINTENANCE_TABS = [
  { id: 'mission' as const, label: 'Mission' },
  { id: 'updates' as const, label: 'Updates' },
  { id: 'schedules' as const, label: 'Schedules' },
]

function missionTimelineProps(steps: MaintenanceMissionHost['steps']) {
  const labels = steps.map((s) => s.label)
  const firstActive = steps.findIndex((s) => s.status === 'ready' || s.status === 'pending' || s.status === 'blocked')
  const allComplete = steps.every((s) => s.status === 'done' || s.status === 'skipped')
  const failed = steps.some((s) => s.status === 'blocked')
  const activeIndex = firstActive < 0 ? (allComplete ? steps.length : 0) : firstActive
  return { labels, activeIndex, allComplete, failed }
}

function stepTone(status: MaintenanceStepStatus): string {
  if (status === 'done') return statusChipClasses('ok')
  if (status === 'ready') return statusChipClasses('info')
  if (status === 'blocked') return statusChipClasses('error')
  if (status === 'skipped') return 'text-[var(--text-muted)] border-white/[0.06]'
  return 'text-[var(--text-muted)] border-white/[0.08]'
}

const SKEW_LABEL: Record<UpgradeHostSkew['status'], string> = {
  current: 'Up to date',
  supported: 'One version behind',
  unsupported: 'Too far behind',
  controller_behind: 'Newer than the controller',
  unknown: 'Version unknown',
}

export default function PlatformMaintenance() {
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [searchParams] = useSearchParams()
  const [tab, setTab] = usePlatformTabState<TabId>(MAINTENANCE_TABS.map((t) => t.id), { defaultTab: 'updates' })

  const [fleet, setFleet] = useState<FleetUpdatesOverview | null>(null)
  const [upgradeMatrix, setUpgradeMatrix] = useState<UpgradeMatrix | null>(null)
  const [mission, setMission] = useState<FleetMaintenanceMissionOverview | null>(null)
  const [rows, setRows] = useState<MaintenanceSchedule[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [error, setError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [pageLoading, setPageLoading] = useState(true)
  const [confirmApplyUpgradeHost, setConfirmApplyUpgradeHost] = useState(false)
  const [confirmApplyAllUpgrades, setConfirmApplyAllUpgrades] = useState(false)
  const [loadingUpdates, setLoadingUpdates] = useState(false)
  const [hostId, setHostId] = useState('')
  const [missionHostId, setMissionHostId] = useState('')
  const [runAt, setRunAt] = useState('')
  const defaultTabSet = useRef(false)
  const [enrollOpen, setEnrollOpen] = useState(false)
  const [previewSummary, setPreviewSummary] = useState<string | null>(null)
  const [upgradeBusy, setUpgradeBusy] = useState(false)

  const loadSchedules = useCallback(async () => {
    const [schedules, hostRows] = await Promise.all([listMaintenanceSchedules(), listPlatformHosts()])
    setRows(schedules)
    setHosts(hostRows)
    setHostId((current) => current || hostRows[0]?.id || '')
  }, [])

  const loadUpdates = useCallback(async () => {
    setLoadingUpdates(true)
    setError(null)
    try {
      const [data, matrix] = await Promise.all([getFleetUpdates(), getUpgradeMatrix()])
      setFleet(data)
      setUpgradeMatrix(matrix)
      if (!defaultTabSet.current && data.hosts_with_updates > 0 && !searchParams.get('tab')) {
        defaultTabSet.current = true
        setTab('mission')
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
      setFleet(null)
      setUpgradeMatrix(null)
    } finally {
      setLoadingUpdates(false)
    }
  }, [searchParams, setTab])

  const loadMission = useCallback(async () => {
    setError(null)
    const data = await getFleetMaintenanceMission()
    setMission(data)
    setMissionHostId((current) => {
      if (!current || !data.hosts.some((h) => h.host_id === current)) {
        return data.hosts[0]?.host_id ?? ''
      }
      return current
    })
  }, [])

  const load = useCallback(async () => {
    setError(null)
    setPageLoading(true)
    try {
      if (tab === 'updates') {
        await loadUpdates()
      } else if (tab === 'mission') {
        await loadMission()
      } else {
        await loadSchedules()
      }
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setPageLoading(false)
    }
  }, [tab, loadUpdates, loadSchedules, loadMission])

  useEffect(() => { void load() }, [load])

  const hostName = (id: string) => hosts.find((h) => h.id === id)?.hostname || id.slice(0, 8)
  const selectedMission = mission?.hosts.find((h) => h.host_id === missionHostId) ?? mission?.hosts[0] ?? null
  const timeline = selectedMission ? missionTimelineProps(selectedMission.steps) : null

  const maintenanceUpgradeTargets = useMemo(
    () => (mission?.hosts ?? []).filter((h) => h.maintenance_mode && (h.pending_packages ?? 0) > 0),
    [mission],
  )

  const runMissionAction = async (label: string, fn: () => Promise<unknown>) => {
    setActionError(null)
    try {
      await fn()
      toast.success(`${label} queued`)
      await loadMission()
    } catch (e: unknown) {
      setActionError(formatUserError(e))
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Maintenance"
      subtitle="Fleet patch catalog, maintenance mission timeline, and deferred windows — guided orchestration only."
      icon={<Download className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >
      <DetailTabs primary={MAINTENANCE_TABS} active={tab} onChange={setTab} />

      {actionError && <ErrorBanner message={actionError} />}
      {pageLoading && <PageSkeleton />}

      {!pageLoading && tab === 'mission' && mission && (
        <div className="space-y-4">
          <p className="text-sm text-[var(--text-muted)]">{mission.summary}</p>
          <div className="apple-metric-band">
            {[
              { label: 'Hosts with updates', value: String(mission.hosts_with_updates) },
              { label: 'In maintenance', value: String(mission.hosts_in_maintenance) },
              { label: 'Pending schedules', value: String(mission.pending_schedules) },
            ].map((s) => (
              <div key={s.label} className="min-w-0">
                <div className="apple-metric-value">{s.value}</div>
                <div className="apple-metric-label">{s.label}</div>
              </div>
            ))}
          </div>

          {mission.hosts.length === 0 ? (
            <PlatformEmptyState icon={ListChecks} title="No hosts" subtitle="Enroll hypervisors to run a maintenance mission.">
              <button type="button" className="tahoe-btn-primary text-sm" onClick={() => setEnrollOpen(true)}>
                Enroll host
              </button>
            </PlatformEmptyState>
          ) : (
            <>
              <div className="flex flex-wrap gap-3 items-center">
                <label htmlFor="maintenance-mission-host" className="text-xs text-[var(--text-muted)]">Host</label>
                <select
                  id="maintenance-mission-host"
                  name="mission_host"
                  className="input max-w-xs"
                  aria-label="Host"
                  value={selectedMission?.host_id ?? ''}
                  onChange={(e) => setMissionHostId(e.target.value)}
                >
                  {mission.hosts.map((h) => (
                    <option key={h.host_id} value={h.host_id}>{h.hostname}</option>
                  ))}
                </select>
                <Link to={`/platform/hosts/${selectedMission?.host_id}`} className={`text-xs ${hubLinkClasses()}`}>
                  Host Linux tab →
                </Link>
                <Link to={operationsHubHref(tier)} className={`text-xs ${hubLinkClasses()}`}>
                  Operations hub →
                </Link>
              </div>

              {selectedMission && timeline && (
                <>
                  <BuildStepTimeline
                    steps={timeline.labels}
                    activeIndex={timeline.activeIndex}
                    allComplete={timeline.allComplete}
                    failed={timeline.failed}
                    variant="amber"
                  />
                  <div className="flex flex-wrap gap-2">
                    {selectedMission.steps.map((s) => (
                      <span key={s.id} className={`text-[10px] uppercase px-2 py-0.5 rounded border ${stepTone(s.status)}`}>
                        {s.label}: {s.status}
                      </span>
                    ))}
                  </div>
                  {selectedMission.blockers.length > 0 && (
                    <p className="text-sm text-amber-700/90">{selectedMission.blockers.join(' ')}</p>
                  )}
                  {selectedMission.update_summary && (
                    <p className="text-xs text-[var(--text-muted)]">{selectedMission.update_summary}</p>
                  )}
                  {!selectedMission.maintenance_mode && (selectedMission.pending_packages ?? 0) > 0 && (
                    <div className={`rounded-lg border p-3 text-sm ${statusSurfaceClasses('warn')}`}>
                      <p className="font-medium">Step blocked — enter maintenance</p>
                      <p className="mt-1 opacity-90">
                        {selectedMission.pending_packages} pending package update(s) cannot be applied until this host is in maintenance mode.
                      </p>
                      <button
                        type="button"
                        className="btn-primary text-xs mt-2"
                        onClick={() => void runMissionAction('Enter maintenance', () =>
                          hostMaintenance(selectedMission.host_id, 'enter', true),
                        )}
                      >
                        Enter maintenance
                      </button>
                    </div>
                  )}

                  <MacGlassPanel title="Operator actions" subtitle="Confirmed steps only — no autonomous package apply.">
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        className="btn-secondary text-sm"
                        onClick={() => {
                          const dt = new Date(Date.now() + 3600_000)
                          const local = new Date(dt.getTime() - dt.getTimezoneOffset() * 60_000)
                            .toISOString()
                            .slice(0, 16)
                          setRunAt(local)
                          setTab('schedules')
                          setHostId(selectedMission.host_id)
                        }}
                      >
                        Open schedules
                      </button>
                      <button
                        type="button"
                        className="btn-primary text-sm"
                        disabled={selectedMission.maintenance_mode}
                        onClick={() => void runMissionAction('Enter maintenance', () =>
                          hostMaintenance(selectedMission.host_id, 'enter', true),
                        )}
                      >
                        Enter maintenance
                      </button>
                      <button
                        type="button"
                        className="btn-secondary text-sm"
                        disabled={!selectedMission.maintenance_mode}
                        onClick={() => void runMissionAction('Exit maintenance', () =>
                          hostMaintenance(selectedMission.host_id, 'exit', false),
                        )}
                      >
                        Exit maintenance
                      </button>
                      {selectedMission.agent_drift && (
                        <button
                          type="button"
                          className="btn-secondary text-sm"
                          onClick={() => void runMissionAction('Agent upgrade', () =>
                            upgradeHostAgent(selectedMission.host_id),
                          )}
                        >
                          Upgrade agent
                        </button>
                      )}
                      <button
                        type="button"
                        className="btn-secondary text-sm"
                        disabled={upgradeBusy || !(selectedMission.pending_packages ?? 0)}
                        onClick={() => {
                          if (!selectedMission) return
                          setUpgradeBusy(true)
                          void previewHostPackageUpgrade(selectedMission.host_id)
                            .then((r) => {
                              setPreviewSummary(r.summary ?? 'Preview complete')
                              toast.success('Package upgrade preview ready')
                            })
                            .catch((e: unknown) => { setPreviewSummary(null); toast.error(formatUserError(e)) })
                            .finally(() => setUpgradeBusy(false))
                        }}
                      >
                        Preview upgrade
                      </button>
                      <button
                        type="button"
                        className="btn-primary text-sm"
                        disabled={!selectedMission.maintenance_mode || !(selectedMission.pending_packages ?? 0)}
                        onClick={() => { if (selectedMission) setConfirmApplyUpgradeHost(true) }}
                      >
                        Apply upgrades
                      </button>
                      {maintenanceUpgradeTargets.length > 1 && (
                        <button
                          type="button"
                          className="btn-secondary text-sm"
                          onClick={() => setConfirmApplyAllUpgrades(true)}
                        >
                          Upgrade all in maintenance
                        </button>
                      )}
                      <Link
                        to={`/platform/hosts/${selectedMission.host_id}?tab=linux`}
                        className="btn-secondary text-sm inline-flex items-center"
                      >
                        Host Linux tab
                      </Link>
                    </div>
                    {previewSummary && (
                      <p className="text-xs text-[var(--text-muted)] mt-3 font-mono whitespace-pre-wrap">{previewSummary}</p>
                    )}
                  </MacGlassPanel>
                </>
              )}
            </>
          )}
        </div>
      )}

      {!pageLoading && tab === 'updates' && (
        <div className="space-y-4">
          {fleet && (
            <>
              <p className="text-sm text-[var(--text-muted)]">{fleet.summary}</p>
              <div className="apple-metric-band">
                {[
                  { label: 'Hosts scanned', value: String(fleet.hosts_scanned) },
                  { label: 'OS updates', value: String(fleet.hosts_with_updates) },
                  { label: 'Reboot required', value: String(fleet.hosts_reboot_required) },
                  { label: 'Agent drift', value: String(fleet.agent_drift_count) },
                ].map((s) => (
                  <div key={s.label} className="min-w-0">
                    <div className="apple-metric-value">{s.value}</div>
                    <div className="apple-metric-label">{s.label}</div>
                  </div>
                ))}
              </div>
              <p className="text-xs text-[var(--text-muted)]">
                Recommended agent: <span className="text-[var(--text-secondary)] font-mono">{fleet.recommended_agent}</span>
                {fleet.total_pending_packages > 0 && (
                  <> · <span className="text-[var(--text-secondary)]">{fleet.total_pending_packages}</span> pending package(s) counted</>
                )}
              </p>
              {upgradeMatrix && (
                <MacGlassPanel title="Agent upgrade matrix" subtitle="Controller vs enrolled agent compatibility">
                  <dl className="grid gap-2 text-sm sm:grid-cols-2">
                    <div>
                      <dt className="text-xs text-[var(--text-muted)]">Controller</dt>
                      <dd className="font-mono text-[var(--text-primary)]">{upgradeMatrix.controller_version}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-[var(--text-muted)]">Recommended agent</dt>
                      <dd className="font-mono text-[var(--text-primary)]">{upgradeMatrix.recommended_agent}</dd>
                    </div>
                    <div>
                      <dt className="text-xs text-[var(--text-muted)]">Minimum agent</dt>
                      <dd className="font-mono text-[var(--text-primary)]">{upgradeMatrix.min_agent}</dd>
                    </div>
                    <div className="sm:col-span-2">
                      <dt className="text-xs text-[var(--text-muted)]">Notes</dt>
                      <dd className="text-[var(--text-secondary)]">{upgradeMatrix.notes}</dd>
                    </div>
                  </dl>
                  {upgradeMatrix.hosts && upgradeMatrix.hosts.length > 0 ? (
                    <div className="mt-4" data-testid="upgrade-skew">
                      <p className="text-xs text-[var(--text-muted)]">
                        Upgrade order: {(upgradeMatrix.order ?? []).join(' → ')}. The controller goes first; an agent may trail it by one minor version.
                      </p>
                      <ul className="mt-2 divide-y divide-[var(--apple-hairline)]">
                        {upgradeMatrix.hosts.map((h) => (
                          <li key={h.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2 text-sm" data-skew={h.status}>
                            <span className="font-medium text-[var(--text-primary)]">{h.hostname}</span>
                            <span className="font-mono text-xs text-[var(--text-muted)]">{h.agent_version || 'version unknown'}</span>
                            <span className={`rounded-full px-2 py-0.5 text-xs ${h.status === 'current' ? 'bg-emerald-500/15 text-emerald-600' : h.status === 'supported' ? 'bg-amber-500/15 text-amber-600' : 'bg-red-500/15 text-red-500'}`} title={h.reason}>
                              {SKEW_LABEL[h.status]}
                            </span>
                            {h.blocker ? <span className="text-xs text-[var(--text-muted)]">{h.blocker}</span> : null}
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                </MacGlassPanel>
              )}
            </>
          )}
          {!fleet && loadingUpdates && (
            <div className="flex items-center gap-2 text-sm text-[var(--text-muted)] py-8">
              <Loader2 className="w-4 h-4 animate-spin" /> Probing host package managers…
            </div>
          )}
          {fleet && (
            <MacGlassPanel title="Host patch catalog" subtitle="Read-only apt/dnf/apk/pacman/zypper probes via enrolled agents.">
              {fleet.hosts.length === 0 ? (
                <p className="text-sm text-[var(--text-muted)]">No online hosts to scan.</p>
              ) : (
                <div className="divide-y divide-white/[0.04] -mx-1">
                  {fleet.hosts.map((h) => (
                    <MacListRow
                      key={h.host_id}
                      title={h.hostname}
                      subtitle={
                        h.summary
                        ?? (h.pending_count != null ? `${h.pending_count} pending (${h.backend})` : h.backend)
                      }
                      href={`/platform/hosts/${h.host_id}`}
                      badge={
                        <span className={`text-[10px] uppercase px-2 py-0.5 rounded border ${
                          h.status === 'ok'
                            ? statusChipClasses('ok', 'border')
                            : h.status === 'unreachable'
                              ? 'text-[var(--text-muted)] border-white/[0.08]'
                              : statusChipClasses('warn', 'border')
                        }`}>
                          {h.reboot_required ? 'reboot' : h.status}
                        </span>
                      }
                      trailing={
                        <div className="flex flex-col items-end gap-1">
                          {h.agent_update_available ? (
                            <button
                              type="button"
                              className="text-[10px] text-[var(--link)] hover:underline"
                              onClick={(e) => {
                                e.preventDefault()
                                void upgradeHostAgent(h.host_id).then((r) => {
                                  toast.success(`Agent upgrade queued (${r.task_id})`)
                                }).catch((err: unknown) => toast.error(formatUserError(err)))
                              }}
                            >
                              Upgrade agent
                            </button>
                          ) : null}
                        </div>
                      }
                    />
                  ))}
                </div>
              )}
              <p className="text-xs text-[var(--text-muted)] mt-4">
                Apply upgrades on-host or use the{' '}
                <button type="button" className="text-orange-600 hover:underline" onClick={() => setTab('mission')}>
                  Maintenance mission
                </button>{' '}
                tab. Package probes may take up to 45s per hypervisor.
              </p>
            </MacGlassPanel>
          )}
        </div>
      )}

      {tab === 'schedules' && !pageLoading && (
        <>
          <div className="card p-4 grid gap-3 md:grid-cols-4">
            <select id="maintenance-schedule-host" name="schedule_host" className="input" aria-label="Host" value={hostId} onChange={(e) => setHostId(e.target.value)}>
              {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
            </select>
            <input id="maintenance-schedule-time" name="scheduled_at" className="input md:col-span-2" type="datetime-local" aria-label="Scheduled date/time" value={runAt} onChange={(e) => setRunAt(e.target.value)} />
            <button
              type="button"
              className="btn-primary text-sm w-fit flex items-center gap-2"
              disabled={!hostId || !runAt}
              data-testid="schedule-maintenance-btn"
              onClick={async () => {
                setActionError(null)
                try {
                  await createMaintenanceSchedule({
                    host_id: hostId,
                    action: 'enter',
                    evacuate: true,
                    run_at: new Date(runAt).toISOString(),
                  })
                  toast.success('Schedule created')
                  await loadSchedules()
                } catch (e: unknown) {
                  setActionError(formatUserError(e))
                }
              }}
            >
              <Plus className="w-4 h-4" /> Schedule
            </button>
          </div>
          {rows.length === 0 ? (
            <PlatformEmptyState
              icon={CalendarClock}
              title="No maintenance windows"
              subtitle="Schedule deferred evacuate or patch windows for hypervisors."
            />
          ) : (
          <div className="card overflow-x-auto">
            <table className="w-full text-sm" aria-label="Maintenance schedules">
              <thead><tr className="text-[var(--text-muted)] border-b border-[var(--apple-hairline)]"><th scope="col" className="p-3 text-left">Host</th><th scope="col" className="p-3">Action</th><th scope="col" className="p-3">Run at</th><th scope="col" className="p-3">Status</th><th scope="col" className="p-3" /></tr></thead>
              <tbody>{rows.map((s) => (
                <tr key={s.id} className="border-b border-[var(--apple-hairline)]">
                  <td className="p-3">
                    <Link to={`/platform/hosts/${s.host_id}`} className={`hover:underline ${hubLinkClasses()}`}>{hostName(s.host_id)}</Link>
                  </td>
                  <td className="p-3">{s.action}{s.evacuate ? ' (evacuate)' : ''}</td>
                  <td className="p-3 text-[var(--text-muted)]">{new Date(s.run_at).toLocaleString()}</td>
                  <td className="p-3">{s.status}</td>
                  <td className="p-3 text-right">
                    {s.status === 'pending' && (
                      <button type="button" className="btn-secondary text-xs" aria-label="Delete" onClick={async () => {
                        try { await deleteMaintenanceSchedule(s.id); toast.success('Cancelled'); await loadSchedules() } catch (e: unknown) { toast.error(formatUserError(e)) }
                      }}><Trash2 className="w-3 h-3 inline" /></button>
                    )}
                  </td>
                </tr>
              ))}</tbody>
            </table>
          </div>
          )}
        </>
      )}
      <HostEnrollWizard open={enrollOpen} onClose={() => { setEnrollOpen(false); void loadSchedules() }} />
      <ConfirmDialog
        open={confirmApplyUpgradeHost}
        title="Apply Package Upgrades"
        message="Apply OS package upgrades on this host? VMs may be affected if packages require service restarts."
        confirmLabel="Apply"
        variant="warning"
        onCancel={() => setConfirmApplyUpgradeHost(false)}
        onConfirm={() => {
          setConfirmApplyUpgradeHost(false)
          if (!selectedMission) return
          void applyHostPackageUpgrade(selectedMission.host_id)
            .then((r) => { toastQueuedOperation(toast, 'Package upgrade queued', r.task_id, tier); return loadMission() })
            .catch((e: unknown) => toast.error(formatUserError(e)))
        }}
      />
      <ConfirmDialog
        open={confirmApplyAllUpgrades}
        title="Upgrade All Hosts in Maintenance"
        message={`Apply upgrades on ${maintenanceUpgradeTargets.length} host(s) in maintenance mode? VMs may be affected.`}
        confirmLabel="Upgrade all"
        variant="warning"
        onCancel={() => setConfirmApplyAllUpgrades(false)}
        onConfirm={() => {
          setConfirmApplyAllUpgrades(false)
          void Promise.all(maintenanceUpgradeTargets.map((h) => applyHostPackageUpgrade(h.host_id)))
            .then((results) => {
              const last = results[results.length - 1]
              if (last) toastQueuedOperation(toast, `Queued ${results.length} upgrade(s)`, last.task_id, tier)
              return loadMission()
            })
            .catch((e: unknown) => toast.error(formatUserError(e)))
        }}
      />
    </PlatformPageChrome>
  )
}
