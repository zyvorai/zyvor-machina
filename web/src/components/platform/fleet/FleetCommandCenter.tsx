// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import {
  Camera,
  ChevronRight,
  Copy,
  Monitor,
  Pause,
  Play,
  Power,
  Sparkles,
  Square,
  Terminal,
  Trash2,
} from 'lucide-react'
import { getConsoleHubPlan, runVmHealthCheck, type ConsoleHubPlan, type HealthIssue } from '../../../api/platform'
import Sparkline from '../../kit/Sparkline'
import { useVmMetricSeries } from '../../../hooks/useVmMetricSeries'
import VmHealthRing from './VmHealthRing'
import { vmHealthScore } from '../../../utils/vmHealthScore'
import VmFeatureChips from './VmFeatureChips'
import GuestAgentSetupDialog from './GuestAgentSetupDialog'
import VmFixItList from './VmFixItList'
import VmStatusBadge from '../../VmStatusBadge'
import { formatVmMemoryGiB } from '../../../utils/vmVisual'
import { useToastContext } from '../../../contexts/ToastContext'
import ConsoleTheatrePreview from './ConsoleTheatrePreview'
import VmConsoleQuickLinks from './VmConsoleQuickLinks'
import { cinemaHubPath, studioHubPath } from '../../../utils/consoleExperienceMode'
import { DetailPanel } from '../DetailPanel'
import type { FleetCommandCenterProps } from './fleetCommandCenterTypes'

export default function FleetCommandCenter({
  selectedVm,
  hosts,
  hostMap,
  onSsh,
  onMigrate,
  onPower,
  onSnapshot,
  onDelete,
  onAdopt,
  showTheatrePreview = false,
  className = '',
  testId = 'fleet-command-center',
}: FleetCommandCenterProps) {
  const toast = useToastContext()
  const [healthScore, setHealthScore] = useState<number | null>(null)
  const [healthLoading, setHealthLoading] = useState(false)
  const [healthIssues, setHealthIssues] = useState<HealthIssue[] | null>(null)
  const [healthTick, setHealthTick] = useState(0)
  const [plan, setPlan] = useState<ConsoleHubPlan | null>(null)
  const [agentSetupOpen, setAgentSetupOpen] = useState(false)
  const vmRunning = selectedVm?.observed_state === 'running'
  const metricSeries = useVmMetricSeries(selectedVm?.id, Boolean(vmRunning) && selectedVm?.inventory_source !== 'kubevirt', null)

  useEffect(() => {
    if (!selectedVm?.id || !vmRunning) {
      setPlan(null)
      return
    }
    let cancelled = false
    void getConsoleHubPlan(selectedVm.id).then((p) => { if (!cancelled) setPlan(p) }).catch(() => { if (!cancelled) setPlan(null) })
    return () => { cancelled = true }
  }, [selectedVm?.id, vmRunning])

  useEffect(() => {
    if (!selectedVm) {
      setHealthScore(null)
      return
    }
    setHealthLoading(true)
    void runVmHealthCheck(selectedVm.id)
      // Number(x) || null would drop a real score of 0 (worst health). Keep 0.
      .then((h) => {
        setHealthScore(vmHealthScore(h))
        setHealthIssues(h.issues ?? [])
      })
      .catch(() => { setHealthScore(null); setHealthIssues(null) })
      .finally(() => setHealthLoading(false))
  }, [selectedVm?.id, healthTick])

  if (!selectedVm) {
    return (
      <DetailPanel
        empty
        emptyMessage="Select a machine to open Command Center"
        className={className}
        testId={testId}
      />
    )
  }

  const vmState = selectedVm.observed_state
  const running = vmState === 'running'
  const paused = vmState === 'paused'
  const shutoff = vmState === 'shutoff' || vmState === 'shut off'

  const primaryPowerAction = running ? (
    <button
      type="button"
      className="btn-secondary text-xs py-1.5 px-3 inline-flex items-center justify-center gap-1.5 border-amber-500/30 text-amber-700 hover:bg-amber-500/10"
      onClick={() => void onPower(selectedVm, 'shutdown')}
    >
      <Power className="w-3.5 h-3.5" /> Shutdown
    </button>
  ) : paused ? (
    <button
      type="button"
      className="btn-secondary text-xs py-1.5 px-3 inline-flex items-center justify-center gap-1.5 border-[var(--apple-hairline)] text-emerald-700 hover:bg-emerald-500/10"
      onClick={() => void onPower(selectedVm, 'resume')}
    >
      <Play className="w-3.5 h-3.5" /> Resume
    </button>
  ) : (
    <button
      type="button"
      className="btn-secondary text-xs py-1.5 px-3 inline-flex items-center justify-center gap-1.5 border-[var(--apple-hairline)] text-emerald-700 hover:bg-emerald-500/10"
      onClick={() => void onPower(selectedVm, 'start')}
    >
      <Play className="w-3.5 h-3.5" /> Start
    </button>
  )

  const latest = metricSeries.length ? metricSeries[metricSeries.length - 1] : null
  const memPct = latest && selectedVm.memory_mib ? Math.round((latest.memory_used_mib / selectedVm.memory_mib) * 100) : null

  const tiles = [
    { key: 'vcpu', label: 'vCPU', value: selectedVm.vcpus != null ? `${selectedVm.vcpus} cores` : '—', accent: '#0a84ff', spark: running ? metricSeries.map((m) => m.cpu_percent) : null, hint: latest && running ? `${Math.round(latest.cpu_percent)}% used` : null },
    { key: 'mem', label: 'Memory', value: formatVmMemoryGiB(selectedVm.memory_mib), accent: '#bf5af2', spark: running ? metricSeries.map((m) => m.memory_used_mib) : null, hint: memPct != null && running ? `${memPct}% used` : null },
    { key: 'ip', label: 'Guest IP', value: selectedVm.guest_ip ? <span className="font-mono">{selectedVm.guest_ip}</span> : '—', accent: '#30d158', spark: null, hint: null },
    { key: 'src', label: 'Source', value: <span className="capitalize">{selectedVm.inventory_source ?? 'libvirt'}</span>, accent: '#ff9f0a', spark: null, hint: null },
  ]

  const stats = (
    <div className="space-y-4">
      <div className="flex items-center gap-4">
        <VmHealthRing score={healthScore} loading={healthLoading} />
        <div className="min-w-0 flex-1">
          <p className="text-[11px] uppercase tracking-[0.12em] font-semibold text-[var(--text-muted)]">Machine</p>
          <p className="text-base font-semibold tracking-tight text-[var(--text-primary)] truncate">{selectedVm.name}</p>
          <p className="text-xs text-[var(--text-secondary)] mt-0.5">
            {healthScore == null ? 'Health unknown' : healthScore >= 80 ? 'Healthy' : healthScore >= 60 ? 'Needs a look' : 'Needs attention'}
          </p>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2.5">
        {tiles.map((t) => (
          <div key={t.key} className="rounded-2xl border border-[var(--apple-hairline)] p-3 min-w-0" style={{ background: `linear-gradient(160deg, color-mix(in srgb, ${t.accent} 10%, transparent), transparent 70%)` }}>
            <p className="text-[11px] font-medium" style={{ color: t.accent }}>{t.label}</p>
            <p className="text-sm font-semibold text-[var(--text-primary)] mt-0.5 truncate">{t.value}</p>
            {t.spark ? (
              <div className="mt-1.5" style={{ color: t.accent }}><Sparkline values={t.spark} width={110} height={22} /></div>
            ) : null}
            {t.hint ? <p className="text-[10px] text-[var(--text-muted)] mt-0.5">{t.hint}</p> : null}
          </div>
        ))}
      </div>

      <VmFeatureChips vm={selectedVm} plan={plan} onAgentSetup={() => setAgentSetupOpen(true)} />

      {running && healthIssues ? (
        <VmFixItList vm={selectedVm} issues={healthIssues} plan={plan} onDone={() => setHealthTick((t) => t + 1)} />
      ) : null}
    </div>
  )

  const controls = (
    <div className="space-y-4">
      <div className="space-y-2">
        <p className="text-xs font-medium text-[var(--text-muted)]">Power</p>
        <div className="flex flex-wrap items-center gap-1.5">
          {primaryPowerAction}
          {(running || paused) && (
            <IconBtn icon={Square} label="Stop" tone="danger" onClick={() => void onPower(selectedVm, 'stop')} />
          )}
          {running && (
            <IconBtn icon={Pause} label="Pause" onClick={() => void onPower(selectedVm, 'pause')} />
          )}
          {!running && !paused && !shutoff && (
            <IconBtn icon={Power} label="Shutdown" onClick={() => void onPower(selectedVm, 'shutdown')} />
          )}
          <IconBtn icon={Camera} label="Snapshot" onClick={() => void onSnapshot(selectedVm)} />
          {selectedVm.inventory_source !== 'kubevirt' && (
            <IconBtn icon={Terminal} label="SSH" onClick={() => onSsh(selectedVm)} />
          )}
          <IconBtn
            icon={Trash2}
            label="Delete VM"
            tone="danger"
            className="ml-auto"
            onClick={() => void onDelete(selectedVm)}
          />
        </div>
      </div>

      {!showTheatrePreview && (
        <div className="space-y-2">
          <p className="text-xs font-medium text-[var(--text-muted)]">Console</p>
          <VmConsoleQuickLinks vmId={selectedVm.id} running={running} />
          <Link to={cinemaHubPath(selectedVm.id)} className="btn-primary text-sm w-full text-center inline-flex items-center justify-center gap-1.5">
            <Monitor className="w-3.5 h-3.5" /> Open Cinema
          </Link>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2">
        {selectedVm.guest_ip && (
          <button
            type="button"
            className="btn-secondary text-xs py-1.5 px-3 inline-flex items-center gap-1"
            onClick={() => {
              void navigator.clipboard.writeText(selectedVm.guest_ip!)
              toast.success('Guest IP copied')
            }}
          >
            <Copy className="w-3.5 h-3.5" /> Copy IP
          </button>
        )}
        {selectedVm.managed === false && (
          <button type="button" className="btn-secondary text-xs py-1.5 px-3" onClick={() => void onAdopt(selectedVm)}>
            Adopt discovered VM
          </button>
        )}
        <Link to={`/platform/vms/${selectedVm.id}`} className="btn-link text-xs inline-flex items-center gap-0.5">
          Open VM detail <ChevronRight className="w-3 h-3" />
        </Link>
      </div>

      {selectedVm.host_id && hosts.length > 1 && (
        <MigratePicker
          vm={selectedVm}
          hosts={hosts}
          hostMap={hostMap}
          onPick={(destId, destName) => onMigrate(selectedVm, destId, destName)}
        />
      )}

      <div
        className="flex gap-2.5 rounded-2xl border p-3 text-xs"
        style={{
          borderColor: 'color-mix(in srgb, #0a84ff 28%, transparent)',
          background: 'linear-gradient(135deg, color-mix(in srgb, #0a84ff 9%, transparent), color-mix(in srgb, #bf5af2 7%, transparent))',
        }}
      >
        <Sparkles className="w-3.5 h-3.5 shrink-0 mt-0.5 text-[#0a84ff]" />
        <p className="text-[var(--text-secondary)]">
          <span className="font-medium text-[var(--text-primary)]">Zyra — </span>
          {healthScore != null && healthScore < 70
            ? 'Health score is low. Review backups and guest agent connectivity.'
            : selectedVm.guest_ip
              ? 'Machine is reachable. Console and SSH are ready.'
              : 'No guest IP yet. Check network and guest tools.'}
        </p>
      </div>
    </div>
  )

  return (
    <DetailPanel
      title="Command Center"
      subtitle={selectedVm.name}
      statusBadge={<VmStatusBadge state={vmState} />}
      className={className}
      testId={testId}
    >
      <div className="cc-grid">
        {showTheatrePreview ? (
          <div className="min-w-0 order-1 lg:order-none">
            <ConsoleTheatrePreview
              vmId={selectedVm.id}
              vmName={selectedVm.name}
              vmState={vmState}
              onStart={!running ? () => void onPower(selectedVm, paused ? 'resume' : 'start') : undefined}
            />
          </div>
        ) : null}
        <div className="min-w-0 space-y-5 order-2 lg:order-none">
          {stats}
          {controls}
        </div>
      </div>
      <GuestAgentSetupDialog
        open={agentSetupOpen}
        onClose={() => setAgentSetupOpen(false)}
        vm={selectedVm}
        osHint={plan?.os_hint}
        sshUser={plan?.ssh_user}
      />
    </DetailPanel>
  )
}

function IconBtn({
  icon: Icon,
  label,
  onClick,
  tone = 'default',
  className = '',
}: {
  icon: typeof Play
  label: string
  onClick: () => void
  tone?: 'default' | 'danger'
  className?: string
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={`btn-secondary !min-h-0 w-7 h-7 !p-0 inline-flex items-center justify-center rounded-full ${
        tone === 'danger' ? 'text-red-600 hover:bg-red-500/10' : ''
      } ${className}`}
    >
      <Icon className="w-3.5 h-3.5" />
    </button>
  )
}

function MigratePicker({
  vm,
  hosts,
  hostMap,
  onPick,
}: {
  vm: FleetCommandCenterProps['selectedVm']
  hosts: FleetCommandCenterProps['hosts']
  hostMap: Map<string, string>
  onPick: (destId: string, destName: string) => void
}) {
  if (!vm) return null
  const candidates = hosts.filter((h) => h.id !== vm.host_id)
  if (candidates.length === 0) return null
  return (
    <div>
      <p className="text-xs font-medium text-[var(--text-muted)] mb-1">Migrate to</p>
      <select
        aria-label="Migrate VM to host"
        className="w-full text-xs rounded-lg bg-[var(--apple-surface)] border border-white/10 px-2 py-1.5"
        defaultValue=""
        onChange={(e) => {
          const id = e.target.value
          if (!id) return
          const host = hosts.find((h) => h.id === id)
          if (host) onPick(id, host.hostname)
          e.target.value = ''
        }}
      >
        <option value="">Select host…</option>
        {candidates.map((h) => (
          <option key={h.id} value={h.id}>
            {h.hostname} ({hostMap.get(h.id) ?? h.id.slice(0, 8)})
          </option>
        ))}
      </select>
    </div>
  )
}
