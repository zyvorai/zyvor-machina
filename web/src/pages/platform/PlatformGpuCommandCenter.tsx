// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { Cpu, Monitor, RefreshCw, Server } from 'lucide-react'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import PlatformPageChrome, { PlatformBackLink, PlatformRefreshButton, platformStatSubtitle } from '../../components/platform/PlatformPageChrome'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import { TahoeTableWrap } from '../../components/platform/tahoe/TahoeListKit'
import { getFleetGpu, listPlatformHosts, type FleetGpuOverview, type GpuProfileKind } from '../../api/platform'
import { getHostGpus, type HostGpuDevice } from '../../api/platformHostGpu'
import { getGpuPlacement } from '../../api/ai'
import { formatUserError } from '../../utils/apiError'
import { hubLinkClasses, statusBadgeClasses, statusToneClass } from '../../utils/semanticColors'

function profileBadge(kind: GpuProfileKind) {
  switch (kind) {
    case 'mig':
      return statusBadgeClasses('info')
    case 'vgpu':
      return statusBadgeClasses('warn')
    case 'cuda':
      return statusBadgeClasses('ok')
    case 'passthrough':
      return statusBadgeClasses('neutral')
    default:
      return statusBadgeClasses('neutral')
  }
}

function profileLabel(kind: GpuProfileKind) {
  switch (kind) {
    case 'mig':
      return 'MIG'
    case 'vgpu':
      return 'vGPU'
    case 'cuda':
      return 'CUDA'
    case 'passthrough':
      return 'Passthrough'
    default:
      return 'GPU'
  }
}

export default function PlatformGpuCommandCenter() {
  const navigate = useNavigate()
  const [overview, setOverview] = useState<FleetGpuOverview | null>(null)
  const [workload, setWorkload] = useState('inference')
  const [placement, setPlacement] = useState<{
    summary: string
    candidates: Array<{ host_id?: string; hostname: string; gpu_capable: boolean; score: number; reason: string }>
  } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [placementBusy, setPlacementBusy] = useState(false)
  const [pciDevices, setPciDevices] = useState<Array<{ host: string; hostId: string; devices: HostGpuDevice[]; summary: string }>>([])
  const [pciLoading, setPciLoading] = useState(false)
  // Last-response-wins guard: `workload` changes on every keystroke, so a slow
  // ranking response for an older query could otherwise land after a newer
  // one and overwrite it with stale placement candidates.
  const placementSeq = useRef(0)

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      setOverview(await getFleetGpu())
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  const loadPlacement = useCallback(async () => {
    const seq = ++placementSeq.current
    const alive = () => seq === placementSeq.current
    setPlacementBusy(true)
    try {
      const p = await getGpuPlacement(workload)
      if (alive()) setPlacement(p)
    } catch {
      if (alive()) setPlacement(null)
    } finally {
      if (alive()) setPlacementBusy(false)
    }
  }, [workload])

  const loadPci = useCallback(async () => {
    setPciLoading(true)
    try {
      const hosts = (await listPlatformHosts()).filter((h) => h.state === 'online')
      // Each host's GPU query is independent — fire concurrently instead of one host at a time.
      const results = await Promise.all(
        hosts.slice(0, 8).map(async (h) => {
          try {
            const g = await getHostGpus(h.id)
            if (g.devices.length > 0) {
              return { host: h.hostname, hostId: h.id, devices: g.devices, summary: g.nvidia_smi_summary }
            }
            return null
          } catch {
            /* host agent may be offline */
            return null
          }
        }),
      )
      setPciDevices(results.filter((r): r is NonNullable<typeof r> => r !== null))
    } finally {
      setPciLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])
  useEffect(() => { void loadPlacement() }, [loadPlacement])
  useEffect(() => { void loadPci() }, [loadPci])

  const hostIdByName = useMemo(() => {
    const map = new Map<string, string>()
    for (const h of overview?.hosts ?? []) map.set(h.hostname, h.host_id)
    return map
  }, [overview?.hosts])

  const createVmOnHost = (hostname: string, hostId?: string) => {
    const id = hostId ?? hostIdByName.get(hostname)
    const params = new URLSearchParams({ create: 'gpu-workload' })
    if (id) params.set('host_id', id)
    navigate(`/platform/vms?${params.toString()}`)
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      contentLoading={loading && !overview}
      prepend={<PlatformBackLink to="/platform/infrastructure" label="Infrastructure" />}
      title="GPU Command Center"
      subtitle={
        <span className="flex flex-col gap-1">
          <span className="text-[var(--text-muted)]">MIG, vGPU, and CUDA placement</span>
          {overview
            ? platformStatSubtitle([
                { label: 'GPU hosts', value: String(overview.gpu_host_count) },
                { label: 'MIG hosts', value: String(overview.mig_hosts) },
                { label: 'vGPU hosts', value: String(overview.vgpu_hosts) },
                { label: 'CUDA ready', value: String(overview.cuda_ready_hosts) },
              ])
            : null}
        </span>
      }
      icon={<Cpu className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={<PlatformRefreshButton onClick={() => void load()} />}
      contentClassName="space-y-4"
    >

        {!loading && overview && overview.gpu_host_count === 0 && (
          <PlatformEmptyState
            icon={Cpu}
            title="No GPU inventory yet"
            subtitle="Tag hosts with gpu, nvidia, mig, vgpu, or cuda on host detail — the discovery agent picks them up automatically."
          >
            <Link to="/platform/hosts" className={`tahoe-btn-ghost text-sm ${hubLinkClasses()}`}>Browse hosts</Link>
            <Link to="/platform/zyra" className="tahoe-btn-primary text-sm">Open Zyra OS</Link>
          </PlatformEmptyState>
        )}

        {!loading && overview && overview.gpu_host_count > 0 && (
          <>
            <p className="text-sm text-[var(--text-muted)]">{overview.summary}</p>

            {overview.profiles.length > 0 && (
              <div className="flex flex-wrap gap-2">
                {overview.profiles.map((p) => (
                  <span key={p.kind} className={`px-3 py-1 rounded-full text-xs border ${profileBadge(p.kind)}`}>
                    {p.label} · {p.host_count} host{p.host_count === 1 ? '' : 's'} · {p.vm_count} VM{p.vm_count === 1 ? '' : 's'}
                  </span>
                ))}
              </div>
            )}

            <MacGlassPanel title="GPU hosts" subtitle="Site, rack, profile, and VM occupancy">
              <TahoeTableWrap>
                <table className="w-full text-sm" aria-label="GPU hosts">
                  <thead>
                    <tr className="text-left text-[var(--text-muted)] border-b border-white/[0.06]">
                      <th scope="col" className="py-2 pr-2">Host</th>
                      <th scope="col" className="py-2 pr-2">Profile</th>
                      <th scope="col" className="py-2 pr-2">Model</th>
                      <th scope="col" className="py-2 pr-2">Site / rack</th>
                      <th scope="col" className="py-2 pr-2">GPU VMs</th>
                      <th scope="col" className="py-2 pr-2">vGPU slices</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(overview.hosts ?? []).map((h) => (
                      <tr key={h.host_id} className="border-b border-white/[0.04] text-[var(--text-primary)]">
                        <td className="py-2 pr-2">
                          <Link to={`/platform/hosts/${h.host_id}`} className={`hover:underline ${hubLinkClasses()}`}>{h.hostname}</Link>
                        </td>
                        <td className="py-2 pr-2">
                          <span className={`text-xs px-2 py-0.5 rounded-full border ${profileBadge(h.profile)}`}>{profileLabel(h.profile)}</span>
                        </td>
                        <td className="py-2 pr-2 text-[var(--text-muted)]">{h.model_hint}</td>
                        <td className="py-2 pr-2 text-[var(--text-muted)] text-xs">{h.site || '—'} / {h.rack || '—'}</td>
                        <td className="py-2 pr-2">{h.gpu_vm_count}</td>
                        <td className="py-2 pr-2">{h.vgpu_slices > 0 ? h.vgpu_slices : '—'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </TahoeTableWrap>
            </MacGlassPanel>

            {(overview.vms?.length ?? 0) > 0 && (
              <MacGlassPanel title="GPU workloads" subtitle="Tagged VMs across the fleet">
                <ul className="text-sm space-y-2 -mt-2">
                  {(overview.vms ?? []).map((v) => (
                    <li key={v.vm_id} className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.04] pb-2">
                      <Link to={`/platform/vms/${v.vm_id}`} className={`inline-flex items-center gap-1.5 hover:underline ${hubLinkClasses()}`}>
                        <Monitor className="w-3.5 h-3.5" />
                        {v.vm_name}
                      </Link>
                      <span className="text-xs text-[var(--text-muted)]">
                        {v.hostname ?? 'unplaced'} · <span className={statusToneClass(v.observed_state === 'running' ? 'ok' : 'neutral')}>{v.observed_state}</span>
                      </span>
                    </li>
                  ))}
                </ul>
              </MacGlassPanel>
            )}

            <MacGlassPanel title="PCI / IOMMU inventory" subtitle="Live discovery via host agent (passthrough & MIG hints)">
              {pciLoading ? (
                <p className="text-sm text-[var(--text-muted)]">Scanning online hosts…</p>
              ) : pciDevices.length === 0 ? (
                <p className="text-sm text-[var(--text-muted)]">No GPU PCI devices reported — check IOMMU and nvidia-smi on hosts.</p>
              ) : (
                <div className="space-y-4 -mt-2">
                  {pciDevices.map((row) => (
                    <div key={row.hostId}>
                      <p className="text-xs font-medium text-[var(--text-secondary)] mb-1">
                        <Link to={`/platform/hosts/${row.hostId}`} className={hubLinkClasses()}>{row.host}</Link>
                      </p>
                      <ul className="text-xs text-[var(--text-muted)] space-y-1">
                        {row.devices.map((d) => (
                          <li key={d.pci_address}>
                            {d.pci_address} · {d.device_name} · IOMMU {d.iommu_group}
                            {d.mig_profile ? ` · MIG ${d.mig_profile}` : ''}
                          </li>
                        ))}
                      </ul>
                    </div>
                  ))}
                </div>
              )}
              <button type="button" className="btn-secondary text-xs mt-3" disabled={pciLoading} onClick={() => void loadPci()}>
                Rescan PCI
              </button>
            </MacGlassPanel>

            <MacGlassPanel title="CUDA placement advisor" subtitle="Rank hosts for inference / training workloads">
              <div className="flex flex-wrap gap-2 items-end -mt-2 mb-3">
                <label className="text-xs text-[var(--text-muted)] flex flex-col gap-1">
                  Workload
                  <input
                    value={workload}
                    onChange={(e) => setWorkload(e.target.value)}
                    className="input text-sm min-w-[200px]"
                    placeholder="inference, training, llama…"
                  />
                </label>
                <button type="button" className="btn-secondary text-sm" disabled={placementBusy} onClick={() => void loadPlacement()}>
                  {placementBusy ? 'Ranking…' : 'Refresh ranking'}
                </button>
                <button type="button" className="btn-secondary text-xs" onClick={() => void load()} aria-label="Refresh inventory">
                  <RefreshCw className="w-4 h-4" />
                </button>
              </div>
              {placement && (
                <>
                  <p className="text-sm text-[var(--text-secondary)] mb-3">{placement.summary}</p>
                  <ul className="text-sm space-y-2">
                    {placement.candidates.map((c) => (
                      <li key={c.hostname} className="flex flex-wrap justify-between gap-2 border-b border-white/[0.04] pb-2 items-center">
                        <span className="inline-flex items-center gap-1.5 text-[var(--text-primary)]">
                          <Server className="w-3.5 h-3.5 shrink-0" />
                          {c.hostname}
                          {c.gpu_capable && <span className={`text-[10px] px-1.5 py-0.5 rounded ${statusBadgeClasses('ok')}`}>GPU</span>}
                        </span>
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-xs text-[var(--text-muted)]">{c.reason} · score {c.score.toFixed(0)}</span>
                          <button
                            type="button"
                            className="btn-secondary text-xs"
                            onClick={() => createVmOnHost(c.hostname, c.host_id)}
                          >
                            Create VM here
                          </button>
                        </div>
                      </li>
                    ))}
                  </ul>
                </>
              )}
            </MacGlassPanel>
          </>
        )}
    </PlatformPageChrome>
  )
}
