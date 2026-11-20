// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useRef, useState } from 'react'
import ConfirmDialog from '../../components/ConfirmDialog'
import { Link, useNavigate, useParams, useLocation, useSearchParams } from 'react-router'
import { ArrowLeft, ExternalLink, Network, Shield, Server, Activity, FileWarning, Bot, Cpu } from 'lucide-react'
import PageLayout from '../../components/PageLayout'
import OsDiagnosePanel from '../../components/platform/OsDiagnosePanel'
import MachinaExplainObjectPanel from '../../components/ai/MachinaExplainObjectPanel'
import HostDetailTabs, { type HostDetailTab } from '../../components/platform/HostDetailTabs'
import {
  MacSettingsGroup,
  MacGlassPanel,
  MacListRow,
} from '../../components/platform/mac/PlatformMacUi'
import ErrorBanner from '../../components/ErrorBanner'
import PageSkeleton from '../../components/PageSkeleton'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import { StructuredErrorBanner } from '../../components/StructuredErrorBanner'
import { hostErrorPresentation } from '../../utils/hostErrorPresentation'
import { getLocalFirewallInventory } from '../../api/advanced'
import JsonInspector from '../../components/platform/JsonInspector'
import {
  getPlatformHostDetail,
  getHostLinuxObservability,
  getHostLinuxUpdates,
  getHostLinuxFilesystems,
  getHostLinuxProcesses,
  previewHostPackageUpgrade,
  applyHostPackageUpgrade,
  rebootHostLinux,
  getHostNetworkDiag,
  getHostLinuxAudit,
  type HostLinuxFilesystem,
  type HostLinuxProcess,
  getHostLldp,
  diagnoseHost,
  hostMaintenance,
  syncHost,
  fenceHost,
  patchHost,
  deleteHost,
  enqueueValidateHost,
  runHostHealthCheck,
  validateHost,
  type PlatformHostDetail,
  type HostLinuxObservability,
  type HostLinuxUpdates,
  type HostNetworkDiag,
  type HostLinuxAuditReport,
  type HostLldpInventory,
  type HostOsDiagnoseReport,
} from '../../api/platform'
import { cordonHost } from '../../api/day2'
import { getFirewallTarget, type FirewallTargetDetail } from '../../api/zeusFirewall'
import { useAi } from '../../contexts/AiContext'
import { useBreadcrumbName } from '../../contexts/BreadcrumbNameContext'
import AskZyraButton from '../../components/ai/AskZyraButton'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError } from '../../utils/apiError'
import { hostStateTone, httpStatusTone, migrationReadinessTone, riskTone, statusBadgeClasses, statusPillClasses, statusSurfaceClasses, statusToneClass, taskStatusTone, utilizationBarClass, webhookDeliveryTone, hubLinkClasses} from '../../utils/semanticColors'
import { openCenterPopout } from '../../utils/platformCenterPopout'
import { hostClassicTools } from '../../utils/platformClassicTools'
import { PlatformClassicToolLinks } from '../../components/platform/PlatformCrossLinks'
import { getHostGpus, type HostGpuDevice } from '../../api/platformHostGpu'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import HostCockpitPanels from '../../components/platform/HostCockpitPanels'
import PlatformHostTerminalPanel from '../../components/platform/PlatformHostTerminalPanel'
import HostPackageKitPanel from '../../components/platform/HostPackageKitPanel'
import HostLibvirtOpsPanel from '../../components/platform/HostLibvirtOpsPanel'
import { getHostCockpitInventory, type HostCockpitSystem } from '../../api/platformHostCockpit'

function psiBar(label: string, pct: number) {
  return (
    <div key={label} className="space-y-1">
      <div className="flex justify-between text-xs text-[var(--text-muted)]">
        <span>{label}</span>
        <span>{pct.toFixed(1)}%</span>
      </div>
      <div className="h-1.5 rounded-full bg-[var(--apple-fill-tertiary)] overflow-hidden">
        <div className={`h-full rounded-full ${utilizationBarClass(pct, { warn: 20, error: 50 })}`} style={{ width: `${Math.min(100, pct)}%` }} />
      </div>
    </div>
  )
}

type HostCheck = { name: string; passed: boolean; message: string; remediation?: string }

const TAB_PARAM: Record<string, HostDetailTab> = {
  general: 'general',
  network: 'network',
  linux: 'linux',
  storage: 'storage',
  system: 'system',
  terminal: 'terminal',
  security: 'security',
  audit: 'audit',
}

export default function PlatformHostDetailPage() {
  const { id } = useParams<{ id: string }>()
  const location = useLocation()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  const [tier] = usePlatformDesktopTier()
  const toast = useToastContext()
  const { openCopilot, setContextHostId } = useAi()
  const [section, setSection] = useState<HostDetailTab>('general')
  const [host, setHost] = useState<PlatformHostDetail | null>(null)
  useBreadcrumbName(host?.hostname ?? '')
  const [linuxObs, setLinuxObs] = useState<HostLinuxObservability | null>(null)
  const [linuxUpdates, setLinuxUpdates] = useState<HostLinuxUpdates | null>(null)
  const [netDiag, setNetDiag] = useState<HostNetworkDiag | null>(null)
  const [audit, setAudit] = useState<HostLinuxAuditReport | null>(null)
  const [lldp, setLldp] = useState<HostLldpInventory | null>(null)
  const [firewall, setFirewall] = useState<FirewallTargetDetail | null>(null)
  const [diagnose, setDiagnose] = useState<HostOsDiagnoseReport | null>(null)
  const [diagnoseLoading, setDiagnoseLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [notes, setNotes] = useState('')
  const [tags, setTags] = useState('')
  // Don't let a load() triggered by an unrelated action wipe unsaved notes.
  const notesDirty = useRef(false)
  // Last-response-wins guards: separate seqs since load + loadOs run concurrently.
  const loadSeq = useRef(0)
  const loadOsSeq = useRef(0)
  const [localFw, setLocalFw] = useState<Record<string, unknown> | null>(null)
  const [site, setSite] = useState('')
  const [rack, setRack] = useState('')
  const [rackU, setRackU] = useState('')
  const [fenceMethod, setFenceMethod] = useState('shell')
  const [ipmiAddress, setIpmiAddress] = useState('')
  const [ipmiUser, setIpmiUser] = useState('')
  const [ipmiPass, setIpmiPass] = useState('')
  const [gpus, setGpus] = useState<HostGpuDevice[]>([])
  const [gpuSummary, setGpuSummary] = useState('')
  const [gpuLoading, setGpuLoading] = useState(false)
  const [gpuError, setGpuError] = useState<string | null>(null)
  const [healthChecks, setHealthChecks] = useState<HostCheck[] | null>(null)
  const [validationChecks, setValidationChecks] = useState<HostCheck[] | null>(null)
  const [opsBusy, setOpsBusy] = useState(false)
  const [filesystems, setFilesystems] = useState<HostLinuxFilesystem[]>([])
  const [processes, setProcesses] = useState<HostLinuxProcess[]>([])
  const [upgradePreview, setUpgradePreview] = useState<string | null>(null)
  const [linuxOpsBusy, setLinuxOpsBusy] = useState(false)
  const [linuxSystemCockpit, setLinuxSystemCockpit] = useState<HostCockpitSystem | null>(null)
  const [cordonBusy, setCordonBusy] = useState(false)
  const [showFenceConfirm, setShowFenceConfirm] = useState(false)
  const [confirmRemoveHost, setConfirmRemoveHost] = useState(false)
  const [confirmForceEvict, setConfirmForceEvict] = useState(false)
  const [confirmApplyUpgrade, setConfirmApplyUpgrade] = useState(false)
  const [confirmRebootHost, setConfirmRebootHost] = useState(false)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: only the newest load may commit so a slow fetch for a
    // prior host can't overwrite the host the user navigated to.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    setLoading(true)
    try {
      const h = await getPlatformHostDetail(id)
      if (!alive()) return
      setHost(h)
      if (!notesDirty.current) setNotes(h.notes || '')
      setTags((h.tags ?? []).join(', '))
      setSite(h.site || '')
      setRack(h.rack || '')
      setRackU(h.rack_u != null ? String(h.rack_u) : '')
    } catch (e: unknown) {
      if (alive()) setError(formatUserError(e))
    } finally {
      if (alive()) setLoading(false)
    }
  }, [id])

  const loadOs = useCallback(async () => {
    if (!id) return
    const seq = ++loadOsSeq.current
    const alive = () => seq === loadOsSeq.current
    if (section === 'network') {
      const [d, l] = await Promise.all([
        getHostNetworkDiag(id).catch(() => null),
        getHostLldp(id).catch(() => null),
      ])
      if (!alive()) return
      setNetDiag(d)
      setLldp(l)
    }
    if (section === 'linux') {
      const [obs, updates, fs, procs, sysCockpit] = await Promise.all([
        getHostLinuxObservability(id).catch(() => null),
        getHostLinuxUpdates(id).catch(() => null),
        getHostLinuxFilesystems(id).catch(() => ({ filesystems: [] })),
        getHostLinuxProcesses(id, 'memory', 15).catch(() => ({ processes: [] })),
        getHostCockpitInventory(id, 'system').then((r) => r.system ?? null).catch(() => null),
      ])
      if (!alive()) return
      setLinuxObs(obs)
      setLinuxUpdates(updates)
      setFilesystems(fs.filesystems ?? [])
      setProcesses(procs.processes ?? [])
      setLinuxSystemCockpit(sysCockpit)
      setGpuLoading(true)
      setGpuError(null)
      try {
        const g = await getHostGpus(id)
        if (alive()) {
          setGpus(g.devices ?? [])
          setGpuSummary(g.nvidia_smi_summary?.trim() ?? '')
        }
      } catch (e: unknown) {
        if (alive()) {
          setGpus([])
          setGpuSummary('')
          setGpuError(formatUserError(e))
        }
      } finally {
        if (alive()) setGpuLoading(false)
      }
    }
    if (section === 'audit') {
      const a = await getHostLinuxAudit(id).catch(() => null)
      if (alive()) setAudit(a)
    }
    if (section === 'security') {
      const f = await getFirewallTarget(id).catch(() => null)
      if (alive()) setFirewall(f)
    }
  }, [id, section])

  const runDiagnose = useCallback(async (query?: string) => {
    if (!id) return
    setDiagnoseLoading(true)
    try {
      setDiagnose(await diagnoseHost(id, query))
    } catch {
      setDiagnose(null)
    } finally {
      setDiagnoseLoading(false)
    }
  }, [id])

  useEffect(() => { void load() }, [load])
  useEffect(() => { void loadOs() }, [loadOs])
  useEffect(() => {
    const tab = searchParams.get('tab')
    if (tab && TAB_PARAM[tab]) setSection(TAB_PARAM[tab])
  }, [searchParams])
  useEffect(() => {
    void getLocalFirewallInventory().then(setLocalFw).catch(() => setLocalFw(null))
  }, [])
  useEffect(() => {
    setContextHostId(id ?? null)
    return () => setContextHostId(null)
  }, [id, setContextHostId])

  if (!id) return null

  const toggleCordon = () => {
    if (!host || cordonBusy) return
    const nextCordon = host.schedulable // cordon when currently schedulable
    setCordonBusy(true)
    void cordonHost(id, nextCordon)
      .then(() => {
        toast.success(nextCordon ? 'Host cordoned — no new VMs will be scheduled' : 'Host uncordoned — scheduling resumed')
        return load()
      })
      .catch((e: unknown) => toast.error(formatUserError(e)))
      .finally(() => setCordonBusy(false))
  }

  // `/proc/pressure/*` avg10 is already a 0-100 percentage (see NodeInfo.tsx's
  // matching, unmultiplied usage) — multiplying by 100 here inflated every
  // reading 100x (e.g. a real 17.67% IO stall showed as "1767%").
  const cpuPsi = linuxObs?.pressure?.cpu?.some ?? 0
  const memPsi = linuxObs?.pressure?.memory?.some ?? 0
  const ioPsi = linuxObs?.pressure?.io?.some ?? 0

  const hostTone = host?.state === 'online' ? 'ok' : host?.state === 'offline' ? 'error' : 'warn'

  return (
    <PageLayout
      compact
      contentLoading={loading && !host}
      prepend={
        <Link to="/platform/hosts" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Hosts
        </Link>
      }
      title={host?.hostname ?? 'Host'}
      subtitle={host ? (
        <span className="flex flex-wrap items-center gap-2 text-sm">
          <span className={statusPillClasses(hostTone)}>{host.state}</span>
          <span className={statusPillClasses(host.validation_status === 'valid' ? 'ok' : 'warn')}>{host.validation_status || 'pending'}</span>
          <span className={statusPillClasses(host.schedulable ? 'ok' : 'warn')}>{host.schedulable ? 'Schedulable' : 'Cordoned'}</span>
          {host.fenced && <span className={statusPillClasses('error')}>Fenced</span>}
          <span className="text-[var(--text-muted)]">·</span>
          <span className="text-[var(--text-muted)]">{host.vm_count ?? 0} VMs</span>
        </span>
      ) : undefined}
      icon={<Server className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={host ? (
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn-secondary text-sm" onClick={() => void syncHost(id).then(() => toast.success('Sync queued')).catch((e: unknown) => toast.error(formatUserError(e)))}>Sync</button>
          <button type="button" className="btn-secondary text-sm" disabled={cordonBusy} onClick={toggleCordon}>
            {host.schedulable ? 'Cordon' : 'Uncordon'}
          </button>
          <AskZyraButton onClick={() => void runDiagnose('host health and pressure')} />
          <Link to={`/platform/zeus/security/firewall/${id}`} className="btn-secondary text-sm inline-flex items-center gap-1">
            <Shield className="w-4 h-4" /> Firewall
          </Link>
        </div>
      ) : undefined}
      contentClassName="space-y-4"
    >
      {error && (hostErrorPresentation(error) ? (
        <StructuredErrorBanner error={hostErrorPresentation(error)!} />
      ) : (
        <ErrorBanner message={error} />
      ))}
      {error && hostErrorPresentation(error)?.error_code === 'host_agent_offline' && (
        <p className="text-xs text-[var(--text-muted)]">
          <Link to="/platform/enroll" className={hubLinkClasses()}>Add Host / re-enroll agent</Link>
          {' · '}
          <Link to="/node" className={hubLinkClasses()}>Classic node tools</Link>
        </p>
      )}
      {host && (
        <>
          <HostDetailTabs
            active={section}
            onChange={(s) => {
              // Write the tab to the URL too, else it diverges from the visible
              // pane and a refresh jumps back to the stale ?tab= value.
              setSection(s)
              const next = new URLSearchParams(searchParams)
              next.set('tab', s)
              setSearchParams(next, { replace: true })
            }}
          />
            {section === 'general' && (
              <>
              <MachinaExplainObjectPanel kind="host" id={id!} name={host.hostname} />
              <MacGlassPanel title="General">
              <div className="space-y-6 xl:space-y-0 xl:columns-2 xl:gap-6 [&>*]:break-inside-avoid xl:[&>*]:mb-6">
                {(host.validation_report?.length ?? 0) > 0 && (
                  <MacSettingsGroup title="Join validation">
                    <ul className="p-3 text-sm space-y-2">
                      {host.validation_report!.map((c) => (
                        <li key={c.name} className={statusToneClass(c.passed ? 'ok' : 'error')}>
                          <span className="font-mono text-xs">{c.name}</span>: {c.message}
                        </li>
                      ))}
                    </ul>
                  </MacSettingsGroup>
                )}
                <MacSettingsGroup title="Actions">
                  <div className="p-3 flex flex-wrap gap-2">
                    <button type="button" className="btn-secondary text-sm" onClick={() => void syncHost(id).then(() => toast.success('Sync queued')).catch((e: unknown) => setError(formatUserError(e)))}>Sync</button>
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      disabled={opsBusy}
                      onClick={() => {
                        setOpsBusy(true)
                        void runHostHealthCheck(id)
                          .then((r) => setHealthChecks(r.checks ?? []))
                          .catch((e: unknown) => toast.error(formatUserError(e)))
                          .finally(() => setOpsBusy(false))
                      }}
                    >
                      Health check
                    </button>
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      disabled={opsBusy}
                      onClick={() => {
                        setOpsBusy(true)
                        void validateHost(id)
                          .then((r) => setValidationChecks(r.checks ?? []))
                          .catch((e: unknown) => toast.error(formatUserError(e)))
                          .finally(() => setOpsBusy(false))
                      }}
                    >
                      Validate now
                    </button>
                    <button type="button" className="btn-secondary text-sm" onClick={() => void enqueueValidateHost(id).then(() => { toast.success('Validation queued'); return load() }).catch((e: unknown) => toast.error(formatUserError(e)))}>Queue validate</button>
                    <button type="button" className="btn-secondary text-sm" onClick={() => void hostMaintenance(id, 'enter').then(() => { toast.success('Maintenance mode entered'); return load() }).catch((e: unknown) => toast.error(formatUserError(e)))}>Maintenance</button>
                    <button type="button" className="btn-destructive text-sm" onClick={() => setShowFenceConfirm(true)}>Fence</button>
                  </div>
                </MacSettingsGroup>
                {healthChecks && (
                  <MacSettingsGroup title="Health check results">
                    <ul className="p-3 text-sm space-y-2">
                      {healthChecks.map((c) => (
                        <li key={c.name} className={statusToneClass(c.passed ? 'ok' : 'error')}>
                          <span className="font-mono text-xs">{c.name}</span>: {c.message}
                          {c.remediation ? <span className="block text-xs text-[var(--text-muted)] mt-0.5">→ {c.remediation}</span> : null}
                        </li>
                      ))}
                    </ul>
                  </MacSettingsGroup>
                )}
                {validationChecks && (
                  <MacSettingsGroup title="Validation results">
                    <ul className="p-3 text-sm space-y-2">
                      {validationChecks.map((c) => (
                        <li key={c.name} className={statusToneClass(c.passed ? 'ok' : 'error')}>
                          <span className="font-mono text-xs">{c.name}</span>: {c.message}
                        </li>
                      ))}
                    </ul>
                  </MacSettingsGroup>
                )}
                <MacSettingsGroup title="Hardware">
                  <div className="p-3 grid gap-2 text-sm md:grid-cols-2">
                    <div>CPU: {host.cpu_model || '—'}</div>
                    <div>Load: {host.cpu_percent?.toFixed(0)}% · {host.memory_used_mib}/{host.memory_total_mib} MiB</div>
                    <div>Libvirt: {host.libvirt_version || '—'}</div>
                    <div>QEMU: {host.qemu_version || '—'}</div>
                    <div className="md:col-span-2 font-mono text-xs text-[var(--text-muted)]">{host.libvirt_uri}</div>
                  </div>
                </MacSettingsGroup>
                <MacSettingsGroup title="Datacenter location">
                  <p className="px-3 pt-3 text-xs text-[var(--text-muted)]">
                    Organize this host on the Mission Control map (site → rack → U position).
                  </p>
                  <div className="p-3 grid gap-3 sm:grid-cols-3">
                    <label htmlFor="host-site" className="space-y-1 text-sm">
                      <span className="text-[var(--text-muted)] text-xs">Site</span>
                      <input id="host-site" name="site" className="input w-full text-sm" value={site} onChange={(e) => setSite(e.target.value)} placeholder="e.g. Pune" />
                    </label>
                    <label htmlFor="host-rack" className="space-y-1 text-sm">
                      <span className="text-[var(--text-muted)] text-xs">Rack</span>
                      <input id="host-rack" name="rack" className="input w-full text-sm" value={rack} onChange={(e) => setRack(e.target.value)} placeholder="e.g. Rack 01" />
                    </label>
                    <label htmlFor="host-rack-u" className="space-y-1 text-sm">
                      <span className="text-[var(--text-muted)] text-xs">Rack U</span>
                      <input id="host-rack-u" name="rack_u" className="input w-full text-sm" type="number" min={1} max={52} value={rackU} onChange={(e) => setRackU(e.target.value)} placeholder="12" />
                    </label>
                  </div>
                  <div className="px-3 pb-3">
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      onClick={() => {
                        const parsedU = rackU.trim() === '' ? null : Number(rackU)
                        void patchHost(id, {
                          site: site.trim(),
                          rack: rack.trim(),
                          rack_u: parsedU != null && !Number.isNaN(parsedU) ? parsedU : null,
                        }).then(() => { toast.success('Location saved'); return load() })
                          .catch((e: unknown) => toast.error(formatUserError(e)))
                      }}
                    >
                      Save location
                    </button>
                  </div>
                </MacSettingsGroup>
                <MacSettingsGroup title="Tags">
                  <div className="p-3 space-y-2">
                    <label htmlFor="host-tags" className="block text-xs text-[var(--text-muted)]">
                      Placement and capability tags
                    </label>
                    <input
                      id="host-tags"
                      name="tags"
                      className="input w-full text-sm"
                      value={tags}
                      onChange={(e) => setTags(e.target.value)}
                      placeholder="gpu, nvidia:a100, cuda"
                    />
                    <p className="text-xs text-[var(--text-muted)]">
                      Comma-separated tags drive GPU, MIG, vGPU, CUDA, and placement discovery.
                    </p>
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      onClick={() => {
                        const next = tags.split(',').map((tag) => tag.trim()).filter(Boolean)
                        void patchHost(id, { tags: next })
                          .then(() => { toast.success('Tags saved'); return load() })
                          .catch((e: unknown) => toast.error(formatUserError(e)))
                      }}
                    >
                      Save tags
                    </button>
                  </div>
                </MacSettingsGroup>
                <MacSettingsGroup title="Notes">
                  <div className="p-3 space-y-2">
                    <textarea id="host-notes" name="notes" className="input min-h-20 text-sm w-full" aria-label="Notes" value={notes} onChange={(e) => { notesDirty.current = true; setNotes(e.target.value) }} />
                    <button type="button" className="btn-secondary text-sm" onClick={() => void patchHost(id, { notes }).then(() => { notesDirty.current = false; toast.success('Notes saved'); return load() }).catch((e: unknown) => toast.error(formatUserError(e)))}>Save</button>
                  </div>
                </MacSettingsGroup>
                <MacSettingsGroup title="Danger zone">
                  <div className="p-3 space-y-2">
                    <p className="text-xs text-[var(--text-muted)]">Removes the host from fleet inventory. VMs must be evacuated first.</p>
                    <button
                      type="button"
                      className="btn-destructive text-sm"
                      disabled={opsBusy || (host.vm_count ?? 0) > 0}
                      onClick={() => setConfirmRemoveHost(true)}
                    >
                      Remove host
                    </button>
                    {(host.vm_count ?? 0) > 0 && (
                      <p className="text-xs text-amber-600/90">Evacuate or migrate VMs before removing this host.</p>
                    )}
                  </div>
                </MacSettingsGroup>
                {id ? (
                  <div className="p-3">
                    <HostLibvirtOpsPanel hostId={id} online={host.state === 'online'} />
                  </div>
                ) : null}
                <MacSettingsGroup title="Classic hypervisor tools">
                  <div className="p-3">
                    <p className="text-xs text-[var(--text-muted)] mb-3 leading-relaxed">
                      Deep libvirt, device passthrough, host SSH, and capability matrix — classic Machina UI on this daemon.
                    </p>
                    <PlatformClassicToolLinks tools={hostClassicTools()} />
                    <Link to={`/node?host=${encodeURIComponent(host.hostname)}`} className={`text-xs inline-block mt-2 ${hubLinkClasses()}`}>Open NodeInfo →</Link>
                    {localFw ? (
                      <div className="mt-3">
                        <p className="text-xs text-[var(--text-muted)] mb-2">Local firewall inventory (daemon fallback)</p>
                        <JsonInspector data={localFw} />
                      </div>
                    ) : null}
                    <Link to="/platform/placement" className={`text-xs inline-block mt-3 ${hubLinkClasses()}`}>
                      HA status & fence events →
                    </Link>
                  </div>
                </MacSettingsGroup>
              </div>
              </MacGlassPanel>
              </>
            )}

            {section === 'storage' && id && (
              <HostCockpitPanels hostId={id} section="storage" classicHostPath={`/platform/hosts/${id}`} />
            )}

            {section === 'system' && id && (
              <HostCockpitPanels hostId={id} section="system" classicHostPath={`/platform/hosts/${id}`} />
            )}

            {section === 'terminal' && host && (
              <PlatformHostTerminalPanel
                hostname={host.hostname}
                address={host.address}
                online={host.state === 'online'}
              />
            )}

            {section === 'network' && (
              <div className="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-6 [&>*]:break-inside-avoid xl:[&>*]:mb-6">
                {id && <HostCockpitPanels hostId={id} section="network" />}
                {netDiag ? (
                  <MacGlassPanel title="systemd networking">
                    <div className="grid gap-2 sm:grid-cols-2 text-sm mb-3 p-3">
                      <div>networkd: <span className={statusToneClass((netDiag.networkd_active ?? netDiag.systemd_networkd_active) ? 'ok' : 'warn')}>{(netDiag.networkd_active ?? netDiag.systemd_networkd_active) ? 'active' : 'inactive'}</span></div>
                      <div>resolved: <span className={statusToneClass(netDiag.resolved_active ? 'ok' : 'warn')}>{netDiag.resolved_active ? 'active' : 'inactive'}</span></div>
                      {netDiag.summary && <p className="sm:col-span-2 text-xs text-[var(--text-muted)]">{netDiag.summary}</p>}
                    </div>
                    {(netDiag.interfaces ?? []).slice(0, 8).map((iface) => (
                      <MacListRow key={iface.name} title={iface.name} subtitle={(iface.addresses ?? []).join(', ') || iface.state || iface.kind || '—'} />
                    ))}
                    {netDiag.networkctl_status_all && (
                      <details className="p-3 text-xs">
                        <summary className="cursor-pointer text-[var(--text-muted)]">networkctl status</summary>
                        <pre className="mt-2 font-mono text-[var(--text-muted)] max-h-80 resize-y overflow-auto whitespace-pre-wrap">{netDiag.networkctl_status_all.slice(0, 6000)}</pre>
                      </details>
                    )}
                    {netDiag.resolvectl_status && (
                      <details className="p-3 text-xs">
                        <summary className="cursor-pointer text-[var(--text-muted)]">resolvectl status</summary>
                        <pre className="mt-2 font-mono text-[var(--text-muted)] max-h-80 resize-y overflow-auto whitespace-pre-wrap">{netDiag.resolvectl_status.slice(0, 4000)}</pre>
                      </details>
                    )}
                  </MacGlassPanel>
                ) : (
                  <PlatformEmptyState
                    icon={Network}
                    title="Network diagnostics unavailable"
                    subtitle="Install or reconnect the host agent to load systemd-networkd and interface data."
                    action={
                      <div className="flex flex-wrap gap-2">
                        <Link to="/platform/enroll" className="btn-primary text-sm">Enroll agent</Link>
                        <Link to="/node" className="btn-secondary text-sm">Classic node tools</Link>
                      </div>
                    }
                  />
                )}
                {lldp?.neighbors && lldp.neighbors.length > 0 && (
                  <MacGlassPanel title={`LLDP (${lldp.source})`}>
                    {lldp.neighbors.slice(0, 10).map((n) => (
                      <MacListRow key={n.local_interface ?? n.chassis_id ?? n.system_name} title={n.system_name || n.chassis_id || 'neighbor'} subtitle={`${n.local_interface} → ${n.port_id || n.port_description}`} />
                    ))}
                  </MacGlassPanel>
                )}
              </div>
            )}

            {section === 'linux' && (
              <div className="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-6 [&>*]:break-inside-avoid xl:[&>*]:mb-6">
                <MacGlassPanel title="GPU inventory" subtitle="PCI passthrough · MIG · CUDA readiness from host agent">
                  {gpuLoading && <p className="text-sm text-[var(--text-muted)]">Scanning GPUs…</p>}
                  {!gpuLoading && gpuError && (
                    <p className="text-sm text-amber-600/90">{gpuError}</p>
                  )}
                  {!gpuLoading && !gpuError && gpus.length === 0 && (
                    <p className="text-sm text-[var(--text-muted)]">No discrete GPUs reported on this host.</p>
                  )}
                  {!gpuLoading && gpus.length > 0 && (
                    <ul className="divide-y divide-white/[0.04] -mx-1">
                      {gpus.map((gpu) => (
                        <MacListRow
                          key={gpu.pci_address}
                          title={gpu.device_name || gpu.pci_address}
                          subtitle={`${gpu.vendor} · ${gpu.pci_address}${gpu.iommu_group != null && gpu.iommu_group >= 0 ? ` · IOMMU ${gpu.iommu_group}` : ''}${gpu.mig_profile ? ` · MIG ${gpu.mig_profile}` : ''}`}
                        />
                      ))}
                    </ul>
                  )}
                  {gpuSummary && (
                    <pre className="mt-3 text-xs font-mono text-[var(--text-muted)] whitespace-pre-wrap max-h-64 overflow-y-auto">{gpuSummary}</pre>
                  )}
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Link to="/platform/gpu" className={`text-xs inline-flex items-center gap-1 ${hubLinkClasses()}`}>
                      <Cpu className="w-3.5 h-3.5" /> GPU Command Center
                    </Link>
                  </div>
                </MacGlassPanel>
                {linuxObs ? (
                  <>
                    <MacGlassPanel title="Pressure stall (PSI)">
                      <div className="space-y-3">
                        {psiBar('CPU some', cpuPsi)}
                        {psiBar('CPU full', linuxObs.pressure?.cpu?.full ?? 0)}
                        {psiBar('Memory some', memPsi)}
                        {psiBar('Memory full', linuxObs.pressure?.memory?.full ?? 0)}
                        {psiBar('I/O some', ioPsi)}
                        {psiBar('I/O full', linuxObs.pressure?.io?.full ?? 0)}
                      </div>
                    </MacGlassPanel>
                    {(linuxObs.disk_io ?? []).length > 0 && (
                      <MacGlassPanel title="Block I/O">
                        {(linuxObs.disk_io ?? []).slice(0, 8).map((d) => (
                          <MacListRow
                            key={d.device}
                            title={d.device}
                            subtitle={`read ${Math.round(d.read_bytes / 1_048_576)} MiB · write ${Math.round(d.write_bytes / 1_048_576)} MiB`}
                          />
                        ))}
                      </MacGlassPanel>
                    )}
                    {filesystems.length > 0 && (
                      <MacGlassPanel title="Filesystems">
                        {filesystems.slice(0, 10).map((f) => (
                          <MacListRow
                            key={f.mount_point}
                            title={f.mount_point}
                            subtitle={`${f.fstype} · ${f.use_percent != null ? f.use_percent.toFixed(0) : '?'}% used`}
                          />
                        ))}
                      </MacGlassPanel>
                    )}
                    {processes.length > 0 && (
                      <MacGlassPanel title="Top processes">
                        {processes.map((p) => (
                          <MacListRow
                            key={p.pid}
                            title={p.command || p.args || `pid ${p.pid}`}
                            subtitle={`${p.user} · ${p.cpu_percent.toFixed(1)}% CPU · ${Math.round(p.rss_kb / 1024)} MiB RSS`}
                          />
                        ))}
                      </MacGlassPanel>
                    )}
                    {linuxObs.bpf && (
                      <MacGlassPanel title="eBPF summary">
                        <p className="text-sm text-[var(--text-muted)] p-3 font-mono text-xs">
                          {JSON.stringify(linuxObs.bpf)}
                        </p>
                      </MacGlassPanel>
                    )}
                    {(linuxObs.thermal ?? []).length > 0 && (
                      <MacGlassPanel title="Thermal">
                        {(linuxObs.thermal ?? []).map((t) => (
                          <MacListRow key={t.sensor} title={t.label || t.sensor} subtitle={`${t.temp_celsius.toFixed(1)}°C`} />
                        ))}
                      </MacGlassPanel>
                    )}
                    {(linuxObs.smart ?? []).length > 0 && (
                      <MacGlassPanel title="Disk SMART">
                        {(linuxObs.smart ?? []).map((d) => (
                          <MacListRow
                            key={d.device}
                            title={d.device}
                            subtitle={d.summary}
                            badge={!d.passed ? <span className={`text-[10px] ${statusToneClass('error')}`}>fail</span> : undefined}
                          />
                        ))}
                      </MacGlassPanel>
                    )}
                    {id && linuxUpdates && (
                      <HostPackageKitPanel
                        hostId={id}
                        updates={linuxUpdates}
                        maintenanceMode={Boolean(host?.maintenance_mode)}
                        packagekit={linuxSystemCockpit?.packagekit}
                        onRefresh={() => void load()}
                      />
                    )}
                    {linuxUpdates && (
                      <MacGlassPanel
                        title="Package updates"
                        subtitle={`${linuxUpdates.backend ?? 'distro'} · ${linuxUpdates.summary ?? ''}${linuxUpdates.reboot_required ? ' · reboot required' : ''}`}
                      >
                        {!host?.maintenance_mode && ((linuxUpdates.pending_count ?? 0) > 0 || linuxUpdates.reboot_required) && (
                          <div className={`mx-3 mt-3 rounded-lg border p-3 text-xs ${statusSurfaceClasses('warn')}`}>
                            <p className="font-medium">Maintenance mode required</p>
                            <p className="mt-1 opacity-90">Enter maintenance before applying package upgrades or rebooting this hypervisor.</p>
                            <button
                              type="button"
                              className="btn-secondary text-xs mt-2"
                              disabled={linuxOpsBusy || !id}
                              onClick={() => {
                                if (!id) return
                                setLinuxOpsBusy(true)
                                void hostMaintenance(id, 'enter')
                                  .then(() => {
                                    toast.success('Maintenance mode entered')
                                    return load()
                                  })
                                  .catch((e: unknown) => toast.error(formatUserError(e)))
                                  .finally(() => setLinuxOpsBusy(false))
                              }}
                            >
                              Enter maintenance
                            </button>
                          </div>
                        )}
                        <div className="p-3 flex flex-wrap gap-2 border-b border-white/[0.06] mb-2">
                          <button
                            type="button"
                            className="btn-secondary text-xs"
                            disabled={linuxOpsBusy}
                            onClick={() => {
                              if (!id) return
                              setLinuxOpsBusy(true)
                              void previewHostPackageUpgrade(id)
                                .then((r) => {
                                  const out = String((r.result as { stdout?: string })?.stdout ?? r.summary ?? 'Preview complete')
                                  setUpgradePreview(out.slice(0, 4000))
                                  toast.success('Upgrade preview ready')
                                })
                                .catch((e: unknown) => toast.error(formatUserError(e)))
                                .finally(() => setLinuxOpsBusy(false))
                            }}
                          >
                            Preview upgrade
                          </button>
                          <button
                            type="button"
                            className="btn-secondary text-xs"
                            disabled={linuxOpsBusy || !host?.maintenance_mode}
                            title={host?.maintenance_mode ? undefined : 'Enter maintenance mode first'}
                            onClick={() => { if (id) setConfirmApplyUpgrade(true) }}
                          >
                            Apply upgrade
                          </button>
                          {linuxUpdates.reboot_required && (
                            <button
                              type="button"
                              className="btn-secondary text-xs"
                              disabled={linuxOpsBusy || !host?.maintenance_mode}
                              onClick={() => { if (id) setConfirmRebootHost(true) }}
                            >
                              Reboot host
                            </button>
                          )}
                        </div>
                        {(linuxUpdates.packages ?? []).length === 0 ? (
                          <p className="text-sm text-[var(--text-muted)] p-3">
                            {(linuxUpdates.pending_count ?? 0) > 0
                              ? `${linuxUpdates.pending_count} pending update(s) — package list not enumerated`
                              : 'No pending updates.'}
                          </p>
                        ) : (
                          <ul className="divide-y divide-white/[0.04] -mx-1">
                            {linuxUpdates.packages!.slice(0, 20).map((p) => (
                              <MacListRow
                                key={p.name}
                                title={p.name}
                                subtitle={[p.current, p.available].filter(Boolean).join(' → ') || 'pending'}
                                badge={p.security ? <span className={`text-[10px] ${statusToneClass('warn')}`}>security</span> : undefined}
                              />
                            ))}
                          </ul>
                        )}
                        {upgradePreview && (
                          <pre className="mt-2 p-2 text-xs font-mono text-[var(--text-muted)] max-h-72 overflow-auto bg-[var(--apple-surface)]/50 rounded-lg">{upgradePreview}</pre>
                        )}
                      </MacGlassPanel>
                    )}
                  </>
                ) : (
                  <PlatformEmptyState
                    icon={Activity}
                    title="Linux observability unavailable"
                    subtitle="PSI, thermal, SMART, and package data require a connected host agent."
                    action={
                      <div className="flex flex-wrap gap-2">
                        <Link to="/platform/enroll" className="btn-primary text-sm">Enroll agent</Link>
                        <Link to="/node" className={`text-sm self-center ${hubLinkClasses()}`}>Classic node tools →</Link>
                      </div>
                    }
                  />
                )}
                <OsDiagnosePanel
                  resourceId={id}
                  resourceKind="host"
                  defaultQuery="why is this host under pressure"
                  loading={diagnoseLoading}
                  report={diagnose}
                  onRun={(q) => void runDiagnose(q)}
                  onAskCopilot={openCopilot}
                />
              </div>
            )}

            {section === 'security' && (
              <div className="space-y-4 xl:space-y-0 xl:columns-2 xl:gap-6 [&>*]:break-inside-avoid xl:[&>*]:mb-6">
                <MacGlassPanel title="Machine Security">
                  {firewall ? (
                    <div className="text-sm space-y-2 mb-3">
                      <div className="flex flex-wrap gap-3 text-[var(--text-secondary)]">
                        <span>Profile: {firewall.target.profile || 'default'}</span>
                        <span>Risk: {firewall.target.risk}</span>
                        <span>Score: {firewall.target.score}</span>
                        <span>{firewall.target.open_ports} open port(s)</span>
                      </div>
                      <p className={`text-xs ${statusToneClass(riskTone(firewall.target.risk))}`}>
                        {firewall.target.agent_reachable ? 'Agent reachable' : 'Agent offline'} · backend {firewall.target.backend}
                      </p>
                    </div>
                  ) : (
                    <p className="text-sm text-[var(--text-muted)] mb-3">Firewall summary unavailable.</p>
                  )}
                  <p className="text-sm text-[var(--text-muted)] mb-3">Zeus Firewall profiles, stealth mode, and port exposure for this hypervisor.</p>
                  <div className="flex flex-wrap gap-2">
                    <Link to={`/platform/zeus/security/firewall/${id}`} className="btn-secondary text-sm inline-flex items-center gap-2">
                      <Shield className="w-4 h-4" /> Open Zeus Firewall
                    </Link>
                    <Link to="/platform/zeus/security/compliance" className="btn-secondary text-sm inline-flex items-center gap-2">
                      Compliance & SIEM
                    </Link>
                    <Link to="/platform/placement" className={`text-sm self-center ${hubLinkClasses()}`}>
                      Fence events →
                    </Link>
                  </div>
                </MacGlassPanel>
                <OsDiagnosePanel
                  resourceId={id}
                  resourceKind="host"
                  defaultQuery="firewall drift on this host"
                  loading={diagnoseLoading}
                  report={diagnose}
                  onRun={(q) => void runDiagnose(q)}
                  onAskCopilot={openCopilot}
                />
              </div>
            )}

            {section === 'audit' && (
              <MacGlassPanel title="Linux audit">
                {audit ? (
                  <div className="text-sm space-y-2 p-3">
                    <p className="text-[var(--text-secondary)]">{audit.summary || 'Audit report loaded'}</p>
                    <div className="grid gap-2 sm:grid-cols-2 text-[var(--text-muted)]">
                      <div>auditd: {audit.auditd_active ? 'active' : 'inactive'}</div>
                      <div>AVC count: {audit.avc_count ?? '—'}</div>
                      <div>recent events: {audit.recent_events ?? audit.events?.length ?? '—'}</div>
                    </div>
                    {(audit.events ?? []).length > 0 && (
                      <ul className="mt-3 space-y-1 font-mono text-[10px] text-[var(--text-muted)]">
                        {audit.events!.slice(0, 20).map((ev, i) => (
                          <li key={String(ev.summary ?? ev.message ?? i)} className="border-b border-white/[0.04] pb-1">{String(ev.summary ?? ev.message ?? JSON.stringify(ev))}</li>
                        ))}
                      </ul>
                    )}
                    <Link to="/platform/zeus/security" className={`text-xs inline-flex mt-2 ${hubLinkClasses()}`}>Security Center →</Link>
                  </div>
                ) : (
                  <p className="text-sm text-[var(--text-muted)] p-3">Audit report unavailable.</p>
                )}
              </MacGlassPanel>
            )}
        </>
      )}
    <ConfirmDialog
      open={confirmRemoveHost}
      title="Remove Host from Fleet"
      message={`Remove host "${host?.hostname}" from fleet inventory? VMs must be evacuated first.`}
      confirmLabel="Remove"
      variant="danger"
      onCancel={() => setConfirmRemoveHost(false)}
      onConfirm={() => {
        setConfirmRemoveHost(false)
        setOpsBusy(true)
        void deleteHost(id!)
          .then(() => { toast.success('Host removed'); navigate('/platform/hosts') })
          .catch((e: unknown) => {
            // Offline/decommissioned host that still owns orphaned VM records: offer
            // a force eviction instead of dead-ending the operator (the API guards
            // against forcing a live host).
            if ((e as { error_code?: string }).error_code === 'host_has_vms') {
              setConfirmForceEvict(true)
            } else {
              toast.error(formatUserError(e))
            }
          })
          .finally(() => setOpsBusy(false))
      }}
    />
    <ConfirmDialog
      open={confirmForceEvict}
      title="Force-evict Host"
      message={`Host "${host?.hostname}" still has VM records assigned but is unreachable. `
        + 'Force-evict it and prune those orphaned VM records from inventory? '
        + 'This only removes controller records — it does not touch any running guests.'}
      confirmLabel="Force evict"
      variant="danger"
      onCancel={() => setConfirmForceEvict(false)}
      onConfirm={() => {
        setConfirmForceEvict(false)
        setOpsBusy(true)
        void deleteHost(id!, true)
          .then((r) => {
            toast.success(`Host evicted${r.vms_pruned ? ` (${r.vms_pruned} VM record(s) pruned)` : ''}`)
            navigate('/platform/hosts')
          })
          .catch((e: unknown) => toast.error(formatUserError(e)))
          .finally(() => setOpsBusy(false))
      }}
    />
    <ConfirmDialog
      open={confirmApplyUpgrade}
      title="Apply Package Upgrades"
      message="Apply all pending package upgrades on this host? This may restart services."
      confirmLabel="Apply"
      variant="warning"
      onCancel={() => setConfirmApplyUpgrade(false)}
      onConfirm={() => {
        setConfirmApplyUpgrade(false)
        setLinuxOpsBusy(true)
        void applyHostPackageUpgrade(id!)
          .then((r) => toastQueuedOperation(toast, r.summary, r.task_id, tier))
          .catch((e: unknown) => toast.error(formatUserError(e)))
          .finally(() => setLinuxOpsBusy(false))
      }}
    />
    <ConfirmDialog
      open={confirmRebootHost}
      title="Reboot Hypervisor"
      message="Reboot this hypervisor now? Running VMs will be suspended or terminated depending on their configuration."
      confirmLabel="Reboot"
      variant="danger"
      onCancel={() => setConfirmRebootHost(false)}
      onConfirm={() => {
        setConfirmRebootHost(false)
        setLinuxOpsBusy(true)
        void rebootHostLinux(id!)
          .then((r) => toastQueuedOperation(toast, r.summary, r.task_id, tier))
          .catch((e: unknown) => toast.error(formatUserError(e)))
          .finally(() => setLinuxOpsBusy(false))
      }}
    />
    <ConfirmDialog
      open={showFenceConfirm}
      title="Fence Host"
      message={`Fence host "${host?.hostname}"? This will forcibly cut power or reset the machine, terminating all running VMs immediately. Only use in an emergency.`}
      confirmLabel="Fence"
      variant="danger"
      onCancel={() => setShowFenceConfirm(false)}
      onConfirm={() => {
        setShowFenceConfirm(false)
        void fenceHost(id!).then(() => { toast.success('Fence invoked'); return load() }).catch((e: unknown) => toast.error(formatUserError(e)))
      }}
    />
    </PageLayout>
  )
}
