// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useRef, useState, useMemo } from 'react'
import { Link } from 'react-router'
import { Monitor, Terminal, Wifi } from 'lucide-react'
import { getPlatformVmMetrics, listVmBackups, runVmHealthCheck, type PlatformVm } from '../../../api/platform'
import type { SecurityReport } from '../../../api/ai'
import { machineAuraClass, machineAuraTone } from '../../../components/consolehub/MachineCanvas'
import VmStatusBadge from '../../../components/VmStatusBadge'
import { guestToolsStatusLabel } from '../../../utils/guestAgentUx'
import { formatVmMemoryGiB, vmLaunchpadGradient, vmSemanticKind } from '../../../utils/vmVisual'
import { statusPillClasses } from '../../../utils/semanticColors'
import { openCenterPopout } from '../../../utils/platformCenterPopout'
import { cinemaHubPath, cinemaPopoutPath } from '../../../utils/consoleExperienceMode'
import type { MachineFinderOverlay } from './machineFinderTypes'

type Props = {
  vm: PlatformVm
  selected: boolean
  overlay: MachineFinderOverlay
  onSelect: () => void
  onDragStart: () => void
  onDragEnd?: () => void
  onSsh: () => void
  onDoubleClickTheatre: () => void
  guestIp?: string
  aiSecurity?: SecurityReport | null
}

export default function LivingMachineCard({
  vm,
  selected,
  overlay,
  onSelect,
  onDragStart,
  onDragEnd,
  onSsh,
  onDoubleClickTheatre,
  guestIp,
  aiSecurity,
}: Props) {
  const rootRef = useRef<HTMLDivElement>(null)
  const [hydrated, setHydrated] = useState(false)
  const [cpuPct, setCpuPct] = useState<number | null>(null)
  const [memPct, setMemPct] = useState<number | null>(null)
  const [healthScore, setHealthScore] = useState<number | null>(null)
  const [lastBackup, setLastBackup] = useState<string | null>(null)
  const [securityRisk, setSecurityRisk] = useState<string | null>(null)
  const [securityFindings, setSecurityFindings] = useState<number | null>(null)

  const running = vmSemanticKind(vm.observed_state) === 'running'
  const libvirt = vm.inventory_source !== 'kubevirt'
  const tone = machineAuraTone(vm.observed_state, healthScore)
  const auraRing = machineAuraClass(tone)
  const hasGpu = useMemo(() => {
    const tags = (vm.tags ?? []).join(' ').toLowerCase()
    return tags.includes('gpu') || tags.includes('nvidia') || tags.includes('cuda') || tags.includes('vgpu')
  }, [vm.tags])

  useEffect(() => {
    const el = rootRef.current
    if (!el) return
    const obs = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) setHydrated(true)
      },
      { rootMargin: '80px' },
    )
    obs.observe(el)
    return () => obs.disconnect()
  }, [])

  useEffect(() => {
    if (!hydrated) return
    const memTotal = vm.memory_mib
    let cancelled = false
    void (async () => {
      try {
        if (overlay === 'default' || overlay === 'health' || overlay === 'cost') {
          const metrics = await getPlatformVmMetrics(vm.id).catch(() => null)
          if (!cancelled && metrics) {
            setCpuPct(metrics.cpu_percent ?? null)
            if (memTotal > 0 && metrics.memory_used_mib) {
              setMemPct(Math.round((metrics.memory_used_mib / memTotal) * 100))
            }
          }
        }
        if (overlay === 'health' || overlay === 'default') {
          const h = await runVmHealthCheck(vm.id).catch(() => null)
          // Number(x) || null drops a legitimate score of 0 (worst health) — the
          // very case that matters most. Keep any finite number, including 0.
          if (!cancelled && h?.score != null) {
            const n = Number(h.score)
            setHealthScore(Number.isFinite(n) ? n : null)
          }
        }
        if (overlay === 'backup' || overlay === 'default') {
          const backups = await listVmBackups(vm.id).catch(() => [])
          if (!cancelled && backups.length > 0) {
            const latest = backups[0]
            setLastBackup(latest.created_at ?? latest.status ?? 'completed')
          }
        }
      } catch {
        /* lazy fetch best-effort */
      }
    })()
    return () => { cancelled = true }
  }, [hydrated, vm.id, overlay])

  useEffect(() => {
    if (overlay !== 'security' || !aiSecurity) return
    setSecurityRisk(aiSecurity.risk_level)
    const needle = vm.name.toLowerCase()
    const related = aiSecurity.findings.filter(
      (f) => f.title.toLowerCase().includes(needle) || f.detail.toLowerCase().includes(needle),
    )
    setSecurityFindings(related.length > 0 ? related.length : aiSecurity.findings.length)
  }, [overlay, aiSecurity, vm.name])

  return (
    <div
      ref={rootRef}
      draggable
      role="button"
      tabIndex={0}
      aria-label={`Machine ${vm.name}`}
      aria-pressed={selected}
      onDragStart={(e) => {
        e.dataTransfer.setData('application/x-platform-vm', vm.id)
        e.dataTransfer.effectAllowed = 'move'
        onDragStart()
      }}
      onDragEnd={() => onDragEnd?.()}
      onClick={onSelect}
      onDoubleClick={(e) => {
        e.preventDefault()
        onDoubleClickTheatre()
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          // First activation selects; activating an already-selected card opens
          // the console theatre (mirrors click → double-click for keyboard users).
          if (selected) onDoubleClickTheatre()
          else onSelect()
        }
      }}
      className={`machine-finder-card cursor-grab active:cursor-grabbing rounded-2xl p-3 transition ${auraRing} ${
        selected ? 'ring-2 ring-[color-mix(in_srgb,var(--accent)_45%,transparent)] bg-[var(--accent)]/5' : 'hover:bg-white/[0.03]'
      }`}
      data-testid={`machine-card-${vm.id}`}
    >
      <div
        className="mx-auto w-20 h-20 sm:w-24 sm:h-24 rounded-2xl flex items-center justify-center mb-2"
        style={{ background: vmLaunchpadGradient(vm.observed_state) }}
      >
        <Monitor className={`w-10 h-10 sm:w-12 sm:h-12 ${running ? 'text-white' : 'text-white/70'}`} />
      </div>

      <div className="text-center space-y-1">
        <p className="font-medium text-sm text-[var(--text-primary)] truncate px-1">{vm.name}</p>
        <VmStatusBadge state={vm.observed_state} />
      </div>

      <dl className="mt-2 space-y-1 text-xs text-[var(--text-muted)]">
        {(overlay === 'default' || overlay === 'network') && (
          <div className="flex justify-between gap-2">
            <dt>IP</dt>
            <dd className="font-mono text-[var(--verdant)] truncate">{guestIp || vm.guest_ip || '—'}</dd>
          </div>
        )}
        {(overlay === 'default' || overlay === 'cost') && (
          <>
            <div className="flex justify-between gap-2">
              <dt>vCPU</dt>
              <dd className="text-[var(--text-primary)]">{vm.vcpus}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt>Mem</dt>
              <dd className="text-[var(--text-primary)]">{formatVmMemoryGiB(vm.memory_mib)}</dd>
            </div>
          </>
        )}
        {overlay === 'health' && healthScore != null && (
          <div className="flex justify-between gap-2">
            <dt>Health</dt>
            <dd className={healthScore >= 85 ? 'text-[var(--verdant)]' : healthScore >= 60 ? 'text-[var(--amber)]' : 'text-[var(--ember)]'}>{healthScore}</dd>
          </div>
        )}
        {overlay === 'backup' && (
          <div className="flex justify-between gap-2">
            <dt>Backup</dt>
            <dd className={lastBackup ? 'text-[var(--verdant)]' : 'text-[var(--amber)]'}>{lastBackup ? 'Recent' : 'At risk'}</dd>
          </div>
        )}
        {overlay === 'security' && (
          <>
            <div className="flex justify-between gap-2">
              <dt>Risk</dt>
              <dd className="text-[var(--amber)] capitalize">{securityRisk ?? '—'}</dd>
            </div>
            <div className="flex justify-between gap-2">
              <dt>Findings</dt>
              <dd className={securityFindings && securityFindings > 0 ? 'text-[var(--amber)]' : 'text-[var(--verdant)]'}>
                {securityFindings ?? '—'}
              </dd>
            </div>
          </>
        )}
        {(overlay === 'gpu' || (overlay === 'default' && hasGpu)) && (
          <div className="flex justify-between gap-2">
            <dt>GPU</dt>
            <dd className={hasGpu ? 'text-[var(--link)]' : 'text-[var(--text-muted)]'}>{hasGpu ? 'Assigned' : 'None'}</dd>
          </div>
        )}
        {overlay === 'default' && libvirt && (
          <div className="flex justify-between gap-2">
            <dt>Agent</dt>
            <dd>
              <span className={statusPillClasses(vm.guest_tools_status === 'healthy' || vm.guest_tools_status === 'installed' ? 'ok' : 'warn')}>
                {guestToolsStatusLabel(vm.guest_tools_status)}
              </span>
            </dd>
          </div>
        )}
        {(overlay === 'default' || overlay === 'migration') && (
          <div className="flex justify-between gap-2">
            <dt>Source</dt>
            <dd className="capitalize truncate">{vm.inventory_source ?? 'libvirt'}</dd>
          </div>
        )}
        {overlay === 'default' && cpuPct != null && (
          <div className="mt-1">
            <div className="flex justify-between text-xs text-[var(--text-muted)] mb-0.5"><span>CPU</span><span>{cpuPct}%</span></div>
            <div className="h-1 rounded-full bg-[var(--mark-track)] overflow-hidden">
              <div className="h-full bg-[var(--accent)]/70" style={{ width: `${Math.min(100, cpuPct)}%` }} />
            </div>
          </div>
        )}
        {overlay === 'default' && memPct != null && (
          <div>
            <div className="flex justify-between text-xs text-[var(--text-muted)] mb-0.5"><span>Mem</span><span>{memPct}%</span></div>
            <div className="h-1 rounded-full bg-[var(--mark-track)] overflow-hidden">
              <div className="h-full bg-[var(--accent)]" style={{ width: `${Math.min(100, memPct)}%` }} />
            </div>
          </div>
        )}
      </dl>

      {running && libvirt && (
        <div className="flex justify-center gap-1 mt-2" onClick={(e) => e.stopPropagation()}>
          <Link to={cinemaHubPath(vm.id, { protocol: 'novnc' })} className="p-1.5 rounded-lg hover:bg-white/10" title="VNC Cinema" aria-label="VNC Cinema"><Monitor className="w-3.5 h-3.5 text-[var(--text-muted)]" /></Link>
          <Link to={cinemaHubPath(vm.id, { protocol: 'spice' })} className="p-1.5 rounded-lg hover:bg-white/10" title="SPICE Cinema" aria-label="SPICE Cinema"><Wifi className="w-3.5 h-3.5 text-[var(--text-muted)]" /></Link>
          <button type="button" className="p-1.5 rounded-lg hover:bg-white/10" title="SSH" aria-label="SSH" onClick={onSsh}><Terminal className="w-3.5 h-3.5 text-[var(--text-muted)]" /></button>
          <button
            type="button"
            className="p-1.5 rounded-lg hover:bg-white/10 text-xs text-[var(--text-muted)]"
            title="Pop out Cinema"
            onClick={() => openCenterPopout(cinemaPopoutPath(vm.id))}
          >
            ⛶
          </button>
        </div>
      )}
    </div>
  )
}
