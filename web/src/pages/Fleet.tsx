// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { RefreshCw, Play, Square, Power, BarChart3 } from 'lucide-react'
import PageLayout from '../components/PageLayout'
import EmptyState from '../components/EmptyState'
import CopyButton from '../components/CopyButton'
import {
  getFleetStatus,
  getFleetVms,
  getFleetMetrics,
  getFleetAlerts,
  postFleetPlacement,
  postFleetCreateVm,
  fleetPeerProxy,
  fleetPrometheusAggregateUrl,
  getFleetPrometheusTargets,
  type FleetPeerStatus,
  type FleetVmRow,
  type FleetMetricsResponse,
  type FleetAlertPeer,
  type PlacementCandidate,
} from '../api/fleet'
import { useToastContext } from '../contexts/ToastContext'
import { formatUserError } from '../utils/apiError'
import VmStatusBadge from '../components/VmStatusBadge'
import { hubLinkClasses, statusToneClass } from '../utils/semanticColors'
import { useTranslation } from 'react-i18next'
import { AppleStoryHeader } from '../components/platform/apple/AppleStoryKit'

export default function FleetPage() {
  const { t } = useTranslation()
  const toast = useToastContext()
  const [loadError, setLoadError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [peers, setPeers] = useState<FleetPeerStatus[]>([])
  const [vms, setVms] = useState<FleetVmRow[]>([])
  const [enabled, setEnabled] = useState(false)
  const [primaryPeer, setPrimaryPeer] = useState('')
  const [standbyPeer, setStandbyPeer] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const [metrics, setMetrics] = useState<FleetMetricsResponse | null>(null)
  const [fleetAlerts, setFleetAlerts] = useState<FleetAlertPeer[]>([])
  const [fleetAlertsTotal, setFleetAlertsTotal] = useState(0)
  const [placementVcpus, setPlacementVcpus] = useState(2)
  const [placementMemMb, setPlacementMemMb] = useState(2048)
  const [placement, setPlacement] = useState<PlacementCandidate[] | null>(null)
  const [placementBusy, setPlacementBusy] = useState(false)
  const [createVmName, setCreateVmName] = useState('')
  const [createBusy, setCreateBusy] = useState(false)
  const [promTargets, setPromTargets] = useState<Awaited<ReturnType<typeof getFleetPrometheusTargets>> | null>(null)
  const [promTargetsBusy, setPromTargetsBusy] = useState(false)

  const load = useCallback(async () => {
    setLoadError(null)
    // Fetch independently: a single down secondary endpoint (metrics/alerts)
    // must not hide an otherwise-healthy fleet. Only a failed core status query
    // surfaces as a page-level error.
    const [stRes, vmRes, metRes, alertsRes] = await Promise.allSettled([
      getFleetStatus(),
      getFleetVms(),
      getFleetMetrics(),
      getFleetAlerts(),
    ])
    if (stRes.status === 'fulfilled') {
      const st = stRes.value
      setEnabled(Boolean(st.enabled))
      setPrimaryPeer(st.primary_peer ?? '')
      setStandbyPeer(st.standby_peer ?? '')
      setPeers(st.peers ?? [])
    } else {
      setLoadError(formatUserError(stRes.reason))
    }
    if (vmRes.status === 'fulfilled') setVms(vmRes.value.vms ?? [])
    if (metRes.status === 'fulfilled') setMetrics(metRes.value)
    if (alertsRes.status === 'fulfilled') {
      setFleetAlerts(alertsRes.value.peers ?? [])
      setFleetAlertsTotal(alertsRes.value.total_unacknowledged ?? 0)
    }
    setLoading(false)
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const peerAction = async (peer: string, vmName: string, action: 'start' | 'stop' | 'shutdown') => {
    const key = `${peer}/${vmName}/${action}`
    setBusy(key)
    try {
      await fleetPeerProxy(peer, 'POST', `/vms/${encodeURIComponent(vmName)}/${action}`)
      toast.success(t('fleet.actionOk', { peer, vm: vmName, action }))
      await load()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(null)
    }
  }

  const reachablePeers = peers.filter((p) => p.reachable).length
  const fleetLede = enabled
    ? [
        primaryPeer ? t('fleet.primaryPeer', { name: primaryPeer }) : null,
        standbyPeer ? t('fleet.standbyPeer', { name: standbyPeer }) : null,
        peers.length > 0 ? `${peers.length} peer${peers.length === 1 ? '' : 's'} · ${reachablePeers} reachable` : null,
        vms.length > 0 ? `${vms.length} VM${vms.length === 1 ? '' : 's'} across fleet` : null,
      ]
        .filter(Boolean)
        .join(' · ')
    : t('fleet.subtitle')

  return (
    <PageLayout
      hideHeader
      className="min-w-0 !space-y-0"
      loading={loading}
      error={loadError}
      errorTitle={t('fleet.title')}
      onErrorRetry={() => void load()}
    >
      <div className="apple-story-stack w-full space-y-8">
        <AppleStoryHeader
          centered
          eyebrow="Fleet"
          title={t('fleet.title')}
          lede={fleetLede}
          cta={
            <button
              type="button"
              onClick={() => void load()}
              className="btn-secondary inline-flex items-center gap-2"
              aria-label={t('common.refresh')}
            >
              <RefreshCw className="w-4 h-4" />
              {t('common.refresh')}
            </button>
          }
        />

        {enabled && metrics ? (
          <section aria-label="Fleet summary" className="apple-section apple-section--tight">
            <div className="apple-metric-band">
              <div className="min-w-0">
                <div className="apple-metric-value">{peers.length}</div>
                <div className="apple-metric-label">{t('fleet.peers')}</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">
                  {reachablePeers} reachable
                </div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">{vms.length}</div>
                <div className="apple-metric-label">{t('fleet.allVms')}</div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">{(metrics.local.host_cpu_percent ?? 0).toFixed(0)}%</div>
                <div className="apple-metric-label">Local CPU</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">
                  Load {(metrics.local.load_1 ?? 0).toFixed(2)}
                </div>
              </div>
              <div className="min-w-0">
                <div className="apple-metric-value">{(metrics.local.host_memory_percent ?? 0).toFixed(0)}%</div>
                <div className="apple-metric-label">Local memory</div>
              </div>
              {fleetAlertsTotal > 0 ? (
                <div className="min-w-0">
                  <div className="apple-metric-value">{fleetAlertsTotal}</div>
                  <div className="apple-metric-label">Alerts</div>
                  <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">Unacknowledged</div>
                </div>
              ) : null}
            </div>
          </section>
        ) : null}

      {!enabled ? (
        <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)] px-4 py-3 space-y-2">
          <p className="text-[var(--text-muted)] text-sm">{t('fleet.disabledHint')}</p>
          <p className="text-xs text-[var(--text-muted)]">
            Enable fleet mode in machina config, or return to the{' '}
            <Link to="/platform" className={hubLinkClasses()}>Platform desktop</Link>
            {' · '}
            <Link to="/platform/settings?section=integrations" className={hubLinkClasses()}>Settings · Integrations</Link>
          </p>
        </div>
      ) : null}

      <section aria-labelledby="fleet-peers-heading" className="apple-section">
        <h2 id="fleet-peers-heading" className="text-lg font-semibold mb-3">
          {t('fleet.peers')}
        </h2>
        {peers.length > 0 ? (
          <div className="overflow-x-auto rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]">
            <table className="w-full text-sm" aria-label={t('fleet.peers')}>
              <thead className="bg-[var(--apple-fill-tertiary)]/60 text-[var(--text-muted)]">
                <tr>
                  <th scope="col" className="px-4 py-2 text-left">Peer</th>
                  <th scope="col" className="px-4 py-2 text-left">URL</th>
                  <th scope="col" className="px-4 py-2 text-left">Status</th>
                  <th scope="col" className="px-4 py-2 text-left">Capacity</th>
                </tr>
              </thead>
              <tbody>
                {peers.map((p) => (
                  <tr key={p.name} className="border-t border-[var(--apple-hairline)]/40">
                    <td className="px-4 py-2 font-medium text-[var(--text-primary)]">{p.name}</td>
                    <td className="px-4 py-2 text-xs text-[var(--text-muted)] truncate max-w-[14rem]" title={p.url}>
                      {p.url}
                    </td>
                    <td className="px-4 py-2">
                      <span className={statusToneClass(p.reachable ? 'ok' : 'warn')}>
                        {p.reachable ? t('fleet.reachable') : t('fleet.unreachable')}
                      </span>
                      {p.version ? (
                        <span className="text-[var(--text-muted)] ml-2">v{p.version}</span>
                      ) : null}
                      {p.error ? (
                        <p className={`text-xs mt-0.5 ${statusToneClass('warn')}`}>{p.error}</p>
                      ) : null}
                    </td>
                    <td className="px-4 py-2 text-xs text-[var(--text-muted)]">
                      {p.vm_count != null ? t('fleet.vmCount', { count: p.vm_count }) : '—'}
                      {p.host_cpu_percent != null ? ` · CPU ${p.host_cpu_percent.toFixed(0)}%` : ''}
                      {p.host_memory_percent != null ? ` · mem ${p.host_memory_percent.toFixed(0)}%` : ''}
                      {p.vms_running != null ? ` · ${p.vms_running} running` : ''}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
        {enabled && peers.length === 0 && !loadError && (
          <EmptyState
            title={t('fleet.noPeersTitle', { defaultValue: 'No fleet peers configured' })}
            description={t('fleet.noPeersHint', { defaultValue: 'Add peer URLs in Settings to aggregate VMs and metrics across Machina nodes.' })}
            primaryAction={
              <Link to="/settings" className="btn-primary text-sm">
                {t('fleet.openSettings', { defaultValue: 'Open Settings' })}
              </Link>
            }
          />
        )}
      </section>

      {enabled ? (
        <section
          className="tahoe-glass-card p-4 space-y-3"
          aria-labelledby="fleet-prometheus-heading"
        >
          <h2
            id="fleet-prometheus-heading"
            className="text-lg font-semibold flex items-center gap-2"
          >
            <BarChart3 className="w-5 h-5 text-[var(--link)]" />
            {t('fleet.prometheusTitle')}
          </h2>
          <p className="text-xs text-[var(--text-muted)]">{t('fleet.prometheusHint')}</p>
          <code className="block text-xs text-[var(--text-secondary)] break-all bg-[var(--apple-surface)] rounded-lg px-3 py-2 border border-[var(--apple-hairline)]/40">
            {fleetPrometheusAggregateUrl()}
          </code>
          <div className="flex flex-wrap gap-2">
            <CopyButton
              text={fleetPrometheusAggregateUrl()}
              label={t('fleet.prometheusCopyUrl')}
            />
            <a
              href={fleetPrometheusAggregateUrl()}
              target="_blank"
              rel="noreferrer"
              className="btn-secondary text-sm"
            >
              {t('fleet.prometheusOpen')}
            </a>
            <button
              type="button"
              data-testid="fleet-prometheus-targets-load"
              disabled={promTargetsBusy}
              className="btn-secondary text-sm"
              onClick={() => {
                setPromTargetsBusy(true)
                void getFleetPrometheusTargets()
                  .then(setPromTargets)
                  .catch((e: unknown) => toast.error(formatUserError(e)))
                  .finally(() => setPromTargetsBusy(false))
              }}
            >
              {promTargetsBusy ? 'Loading…' : 'Load scrape targets'}
            </button>
          </div>
          {promTargets && (
            <div className="text-xs text-[var(--text-muted)] space-y-1" data-testid="fleet-prometheus-targets">
              <p>{promTargets.note}</p>
              <p>
                Metrics path: <code className="text-[var(--text-secondary)]">{promTargets.metrics_path}</code> ·{' '}
                {promTargets.scrape_configs.length} scrape config(s)
              </p>
            </div>
          )}
        </section>
      ) : null}

      <section className="tahoe-glass-card p-4 space-y-3">
        <h2 className="text-lg font-semibold">VM placement</h2>
        <p className="text-xs text-[var(--text-muted)]">
          Rank hypervisors by capacity headroom (CPU, memory, disk) minus requested VM size.
        </p>
        <div className="flex flex-wrap gap-3 items-end text-sm">
          <label>
            <span className="text-xs text-[var(--text-muted)]">vCPUs</span>
            <input
              type="number"
              min={1}
              className="input-field block mt-1 w-20"
              value={placementVcpus}
              onChange={(e) => setPlacementVcpus(Number(e.target.value) || 1)}
            />
          </label>
          <label>
            <span className="text-xs text-[var(--text-muted)]">Memory (MiB)</span>
            <input
              type="number"
              min={256}
              className="input-field block mt-1 w-28"
              value={placementMemMb}
              onChange={(e) => setPlacementMemMb(Number(e.target.value) || 256)}
            />
          </label>
          <button
            type="button"
            disabled={placementBusy}
            className="btn-secondary text-sm"
            onClick={async () => {
              setPlacementBusy(true)
              try {
                const res = await postFleetPlacement(placementVcpus, placementMemMb)
                setPlacement(res.candidates ?? [])
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              } finally {
                setPlacementBusy(false)
              }
            }}
          >
            {placementBusy ? 'Ranking…' : 'Suggest peer'}
          </button>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-[var(--text-muted)]">VM name (peer create)</span>
            <input
              className="input-field w-40"
              value={createVmName}
              onChange={(e) => setCreateVmName(e.target.value)}
              placeholder="my-vm"
            />
          </label>
          <button
            type="button"
            disabled={createBusy || !createVmName.trim()}
            className="btn-secondary text-sm"
            onClick={async () => {
              setCreateBusy(true)
              try {
                const res = await postFleetCreateVm(
                  {
                    name: createVmName.trim(),
                    vcpus: placementVcpus,
                    memory_mb: placementMemMb,
                    disk_gb: 20,
                  },
                  {
                    autoPlace: true,
                    placementVcpus,
                    placementMemoryMb: placementMemMb,
                  },
                )
                if (res.action === 'create_local') {
                  toast.info(res.message ?? 'Create this VM on the local host via VMs → Create')
                } else if (typeof res.status === 'number' && res.status >= 400) {
                  toast.error(`Create failed on ${res.peer} (HTTP ${res.status})${res.message ? `: ${res.message}` : ''}`)
                } else {
                  toast.success(`Create proxied to ${res.peer} (HTTP ${res.status ?? '?'})`)
                }
                await load()
              } catch (e: unknown) {
                toast.error(formatUserError(e))
              } finally {
                setCreateBusy(false)
              }
            }}
          >
            {createBusy ? 'Creating…' : 'Create on best peer'}
          </button>
        </div>
        {placement && placement.length > 0 ? (
          <ul className="text-xs text-[var(--text-muted)] space-y-1">
            {placement.map((c) => (
              <li key={c.peer}>
                <span className={c.recommended ? `${statusToneClass('ok')} font-medium` : ''}>
                  {c.peer}
                  {c.recommended ? ' ← recommended' : ''}
                </span>
                {c.reachable && c.capacity.adjusted_score != null
                  ? ` · score ${c.capacity.adjusted_score} (${c.capacity.label})`
                  : ''}
                {c.error ? ` · ${c.error}` : ''}
              </li>
            ))}
          </ul>
        ) : null}
      </section>

      {metrics ? (
        <section className="apple-section">
          <h2 className="text-lg font-semibold mb-3">Fleet capacity</h2>
          <div className="apple-metric-band">
            <div className="min-w-0">
              <div className="apple-metric-value">{(metrics.local.host_cpu_percent ?? 0).toFixed(1)}%</div>
              <div className="apple-metric-label">Local CPU</div>
            </div>
            <div className="min-w-0">
              <div className="apple-metric-value">{(metrics.local.host_memory_percent ?? 0).toFixed(1)}%</div>
              <div className="apple-metric-label">Local memory</div>
            </div>
            {metrics.local.host_disk_percent != null ? (
              <div className="min-w-0">
                <div className="apple-metric-value">{metrics.local.host_disk_percent.toFixed(1)}%</div>
                <div className="apple-metric-label">Local disk</div>
              </div>
            ) : null}
            <div className="min-w-0">
              <div className="apple-metric-value">{metrics.local.vms_running}/{metrics.local.vm_count}</div>
              <div className="apple-metric-label">Local VMs running</div>
            </div>
            <div className="min-w-0">
              <div className="apple-metric-value">{(metrics.local.load_1 ?? 0).toFixed(2)}</div>
              <div className="apple-metric-label">Load (1m)</div>
            </div>
            {metrics.local.capacity ? (
              <div className="min-w-0">
                <div className="apple-metric-value">{metrics.local.capacity.score}</div>
                <div className="apple-metric-label">Capacity score</div>
                <div className="mt-1 text-[13px] text-[var(--text-muted)] leading-snug">{metrics.local.capacity.label}</div>
              </div>
            ) : null}
          </div>
          {metrics.peers.some((p) => p.capacity) ? (
            <ul className="mt-3 text-xs text-[var(--text-muted)] space-y-1">
              {metrics.peers
                .filter((p) => p.capacity)
                .map((p) => (
                  <li key={p.name}>
                    {p.name}: capacity {p.capacity!.score} ({p.capacity!.label})
                    {p.host_cpu_percent != null
                      ? ` · CPU ${p.host_cpu_percent.toFixed(0)}%`
                      : ''}
                  </li>
                ))}
            </ul>
          ) : null}
        </section>
      ) : null}

      {fleetAlerts.length > 0 ? (
        <section className="tahoe-glass-card p-4">
          <h2 className="text-lg font-semibold mb-2">
            Fleet alerts
            {fleetAlertsTotal > 0 ? (
              <span className={`ml-2 text-sm font-normal ${statusToneClass('warn')}`}>
                {fleetAlertsTotal} unacknowledged
              </span>
            ) : null}
          </h2>
          <div className="space-y-3 text-sm">
            {fleetAlerts.map((row) => (
              <div key={row.peer}>
                <div className="font-medium text-[var(--text-primary)]">{row.peer}</div>
                {row.error ? (
                  <p className={`text-xs ${statusToneClass('warn')}`}>{row.error}</p>
                ) : row.alerts.length === 0 ? (
                  <p className="text-xs text-[var(--text-muted)]">No alerts</p>
                ) : (
                  <ul className="mt-1 text-xs text-[var(--text-muted)] list-disc pl-4">
                    {row.alerts
                      .filter((a) => !a.acknowledged)
                      .slice(0, 5)
                      .map((a) => (
                        <li key={a.id}>
                          [{a.severity}] {a.message}
                        </li>
                      ))}
                  </ul>
                )}
              </div>
            ))}
          </div>
        </section>
      ) : null}

      <section aria-labelledby="fleet-vms-heading">
        <h2 id="fleet-vms-heading" className="text-lg font-semibold mb-3">
          {t('fleet.allVms')}
        </h2>
        <div className="overflow-x-auto apple-surface rounded-2xl/50">
          <table className="w-full text-sm" aria-label="Fleet hosts">
            <thead className="bg-[var(--apple-fill-tertiary)]/60 text-[var(--text-muted)]">
              <tr>
                <th scope="col" className="px-4 py-2 text-left">{t('fleet.colName')}</th>
                <th scope="col" className="px-4 py-2 text-left">{t('fleet.colPeer')}</th>
                <th scope="col" className="px-4 py-2 text-left">{t('fleet.colState')}</th>
                <th scope="col" className="px-4 py-2 text-right">{t('fleet.colActions')}</th>
              </tr>
            </thead>
            <tbody>
              {vms.map((vm) => (
                <tr key={`${vm.peer}/${vm.name}`} className="border-t border-[var(--apple-hairline)]/40">
                  <td className="px-4 py-2">
                    {vm.peer === 'local' ? (
                      <Link
                        to={`/vms/${encodeURIComponent(vm.name)}`}
                        className={`${hubLinkClasses()} hover:underline`}
                      >
                        {vm.name}
                      </Link>
                    ) : (
                      vm.name
                    )}
                  </td>
                  <td className="px-4 py-2 text-[var(--text-muted)]">{vm.peer}</td>
                  <td className="px-4 py-2">
                    <VmStatusBadge state={vm.state} />
                  </td>
                  <td className="px-4 py-2 text-right">
                    {vm.peer !== 'local' ? (
                      <div className="flex justify-end gap-1">
                        <button
                          type="button"
                          disabled={busy != null}
                          onClick={() => void peerAction(vm.peer, vm.name, 'start')}
                          className={`p-1 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-ok)_25%,transparent)]`}
                          title={t('fleet.start')}
                          aria-label={t('fleet.start')}
                        >
                          <Play className="w-4 h-4" />
                        </button>
                        <button
                          type="button"
                          disabled={busy != null}
                          onClick={() => void peerAction(vm.peer, vm.name, 'shutdown')}
                          className={`p-1 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-warn)_25%,transparent)]`}
                          title={t('fleet.shutdown')}
                          aria-label={t('fleet.shutdown')}
                        >
                          <Power className="w-4 h-4" />
                        </button>
                        <button
                          type="button"
                          disabled={busy != null}
                          onClick={() => void peerAction(vm.peer, vm.name, 'stop')}
                          className={`p-1 rounded hover:bg-[color-mix(in_srgb,var(--machina-status-error)_25%,transparent)]`}
                          title={t('fleet.stop')}
                          aria-label={t('fleet.stop')}
                        >
                          <Square className="w-4 h-4" />
                        </button>
                      </div>
                    ) : (
                      <span className="text-[var(--text-faint)] text-xs">{t('fleet.localHost')}</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {vms.length === 0 ? (
            <p className="p-4 text-[var(--text-muted)] text-sm">{t('fleet.noVms')}</p>
          ) : null}
        </div>
      </section>
      </div>
    </PageLayout>
  )
}
