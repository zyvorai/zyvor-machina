// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router'
import { Activity } from 'lucide-react'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import OperatingSurfaceLayout from '../../components/platform/OperatingSurfaceLayout'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import MigratePrecheckModal from '../../components/platform/MigratePrecheckModal'
import {
  getClusterSettings,
  getHaStatus,
  getPlatformVm,
  getPlacementRecommendations,
  listFenceEvents,
  listMigrationJobs,
  patchClusterSettings,
  refreshPlacement,
  type ClusterSettings,
  type FenceEvent,
  type HaStatusResponse,
  type MigrationJob,
  type PlacementRecommendation,
  type PlatformVm,
} from '../../api/platform'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import {statusToneClass, hubLinkClasses} from '../../utils/semanticColors'

export default function PlatformPlacement() {
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [rows, setRows] = useState<PlacementRecommendation[]>([])
  const [ha, setHa] = useState<HaStatusResponse | null>(null)
  const [settings, setSettings] = useState<ClusterSettings | null>(null)
  const [migrations, setMigrations] = useState<MigrationJob[]>([])
  const [fences, setFences] = useState<FenceEvent[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [migrateModal, setMigrateModal] = useState<{ vm: PlatformVm; destId: string; destName: string } | null>(null)
  const [migrateLoading, setMigrateLoading] = useState<string | null>(null)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [recs, haSt, cluster, mig, fence] = await Promise.all([
        getPlacementRecommendations(),
        getHaStatus(),
        getClusterSettings(),
        listMigrationJobs(),
        listFenceEvents(),
      ])
      setRows(recs)
      setHa(haSt)
      setSettings(cluster)
      setMigrations(mig)
      setFences(fence)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const toggleAutoMigrate = async () => {
    if (!settings) return
    try {
      const next = await patchClusterSettings({ drs_auto_migrate: !settings.drs_auto_migrate })
      setSettings(next)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  const updateThreshold = async (v: number) => {
    if (!settings) return
    try {
      setSettings(await patchClusterSettings({ drs_cpu_threshold: v }))
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  const setPlacementPolicy = async (policy: string) => {
    if (!settings) return
    try {
      setSettings(await patchClusterSettings({ placement_policy: policy }))
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }

  useEffect(() => {
    void load()
  }, [load])

  const openMigrate = async (r: PlacementRecommendation) => {
    setMigrateLoading(r.vm_id)
    try {
      const vm = await getPlatformVm(r.vm_id)
      setMigrateModal({ vm, destId: r.to_host_id, destName: r.to_host_name })
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setMigrateLoading(null)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      loading={loading && !settings && !ha}
      prepend={<PlatformBackLink to="/platform/operations" label="Operations" />}
      title="Placement & HA"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">DRS-style recommendations and high-availability status</span>
          {ha && platformStatSubtitle([
            { label: 'HA-enabled VMs', value: ha.status.enabled_vms },
            { label: 'Offline hosts', value: ha.status.offline_hosts },
            { label: 'HA events (24h)', value: ha.status.recent_events },
          ])}
        </span>
      }
      icon={<Activity className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={
        <>
          <button type="button" className="btn-secondary text-xs" onClick={async () => {
            try { setRows(await refreshPlacement()) } catch (e: unknown) { setError(formatUserError(e)) }
          }}>Recompute</button>
          <PlatformRefreshButton onClick={() => void load()} />
        </>
      }
      contentClassName="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-4 [&>*]:break-inside-avoid xl:[&>*]:mb-4"
    >
      <OperatingSurfaceLayout testId="platform-placement-page">
      {settings && (
        <MacGlassPanel title="DRS auto-migrate" subtitle={`When enabled, the controller queues live migrations for overloaded hosts (CPU/memory > ${settings.drs_cpu_threshold}%).`}>
        <section className="flex flex-wrap items-center justify-between gap-4">
          <button type="button" className={`text-sm ${settings.drs_auto_migrate ? 'btn-primary' : 'btn-secondary'}`} onClick={() => void toggleAutoMigrate()}>
            {settings.drs_auto_migrate ? 'Enabled' : 'Disabled'}
          </button>
          <label className="text-sm w-full mt-3 block">
            CPU/memory threshold: {settings.drs_cpu_threshold}%
            <input type="range" min={50} max={95} value={settings.drs_cpu_threshold}
              onChange={(e) => void updateThreshold(Number(e.target.value))} className="w-full mt-1 h-11" />
          </label>
          <label className="text-sm w-full mt-3 block">
            Placement policy
            <select className="input mt-1 block w-48" value={settings.placement_policy}
              onChange={(e) => void setPlacementPolicy(e.target.value)}>
              <option value="balanced">Balanced</option>
              <option value="packed">Packed</option>
            </select>
          </label>
        </section>
        </MacGlassPanel>
      )}
      <MacGlassPanel title="Placement recommendations">
        {rows.length === 0 ? (
          <PlatformEmptyState
            icon={Activity}
            title="Cluster load is balanced"
            subtitle="No DRS migration recommendations — hosts are within threshold."
          />
        ) : (
          <ul className="space-y-3 text-sm">
            {rows.map((r) => (
              <li key={`${r.vm_id}-${r.to_host_id}`} className="border-b border-[var(--apple-hairline)] pb-3">
                <div className="flex flex-wrap justify-between gap-4 items-start">
                  <Link to={`/platform/vms/${r.vm_id}`} className={`font-medium ${hubLinkClasses()}`}>{r.vm_name}</Link>
                  <div className="flex items-center gap-2 shrink-0">
                    <span className="text-[var(--text-muted)]">score {r.score.toFixed(1)}</span>
                    <button
                      type="button"
                      className="btn-primary text-xs"
                      disabled={migrateLoading === r.vm_id}
                      onClick={() => void openMigrate(r)}
                    >
                      {migrateLoading === r.vm_id ? 'Loading…' : 'Migrate'}
                    </button>
                  </div>
                </div>
                <p className="text-[var(--text-muted)] mt-1">{r.from_host_name} → {r.to_host_name}</p>
                <p className="text-[var(--text-muted)] mt-1">{r.reason}</p>
              </li>
            ))}
          </ul>
        )}
      </MacGlassPanel>
      {migrations.length > 0 && (
        <MacGlassPanel title="Recent migrations">
          <ul className="space-y-2 text-sm text-[var(--text-muted)]">{migrations.slice(0, 10).map((m) => (
            <li key={m.id}>{m.status} · VM {m.vm_id.slice(0, 8)} · {m.progress}%</li>
          ))}</ul>
        </MacGlassPanel>
      )}
      {fences.length > 0 && (
        <MacGlassPanel title="Fence events">
          <ul className="space-y-2 text-sm text-[var(--text-muted)]">{fences.slice(0, 10).map((f) => (
            <li key={f.id} className={statusToneClass(f.success ? 'ok' : 'error')}>
              host {f.host_id.slice(0, 8)} — {f.message || f.action}
            </li>
          ))}</ul>
        </MacGlassPanel>
      )}
      {ha && ha.events.length > 0 && (
        <MacGlassPanel title="Recent HA events">
          <ul className="space-y-2 text-sm text-[var(--text-muted)]">
            {ha.events.slice(0, 15).map((e) => (
              <li key={e.id}><span className="text-[var(--text-secondary)]">{e.action}</span> — {e.message}</li>
            ))}
          </ul>
        </MacGlassPanel>
      )}
      </OperatingSurfaceLayout>
      {migrateModal && (
        <MigratePrecheckModal
          vm={migrateModal.vm}
          destHostId={migrateModal.destId}
          destHostName={migrateModal.destName}
          onClose={() => setMigrateModal(null)}
          onDone={(taskId) => {
            if (taskId) toastQueuedOperation(toast, `Migrating ${migrateModal.vm.name}`, taskId, tier)
            else toast.success('Migration queued')
            void load()
          }}
        />
      )}
    </PlatformPageChrome>
  )
}
