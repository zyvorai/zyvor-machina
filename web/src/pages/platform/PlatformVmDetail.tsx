// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useLocation, useNavigate, useParams, useSearchParams } from 'react-router'
import { ArrowLeft, Copy, Play, Square, RotateCcw, Trash2, Terminal, MoveRight, Archive, HardDrive, Activity, Shield, ExternalLink, Monitor, Pause, Power, Server, Loader2, Network, ToggleLeft, ToggleRight, FolderOpen, Cpu } from 'lucide-react'
import PageLayout from '../../components/PageLayout'
import ConfirmDialog from '../../components/ConfirmDialog'
import GuestToolsStrip from '../../components/platform/GuestToolsStrip'
import GuestAgentDiagnosticsPanel, {
  GuestAgentHeaderPill,
  installStateTone,
  type RunGuestActionFn,
} from '../../components/platform/GuestAgentDiagnosticsPanel'
import GuestkitOfflineAssurancePanel from '../../components/platform/GuestkitOfflineAssurancePanel'
import GuestFsFreezeBanner from '../../components/platform/GuestFsFreezeBanner'
import GuestObservabilityStrip from '../../components/platform/GuestObservabilityStrip'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import GuestAiInsightsPanel from '../../components/platform/GuestAiInsightsPanel'
import { getVmGuestAiInsights } from '../../api/platform'
import { deleteK8sKubevirtVm, postK8sKubevirtVmLifecycle } from '../../api/k8s'
import MachinaVmOutageRca from '../../components/ai/MachinaVmOutageRca'
import MachinaExplainObjectPanel from '../../components/ai/MachinaExplainObjectPanel'
import MachinaVmTroubleshootPanel from '../../components/ai/MachinaVmTroubleshootPanel'
import AiTerminalSuggestStrip from '../../components/ai/AiTerminalSuggestStrip'
import VmOverviewTroubleshootPanel from '../../components/ai/VmOverviewTroubleshootPanel'
import MachinaDoctorPanel from '../../components/platform/MachinaDoctorPanel'
import ExplainButton from '../../components/ai/ExplainButton'
import OsDiagnosePanel from '../../components/platform/OsDiagnosePanel'
import VmDetailTabs, { type VmDetailTab, isGuestRelatedTab } from '../../components/platform/VmDetailTabs'
import { MacGlassPanel, MacListRow } from '../../components/platform/mac/PlatformMacUi'
import VmUsageBars from '../../components/platform/VmUsageBars'
import VmOverviewStats from '../../components/platform/vmdetail/VmOverviewStats'
import VmPerfPanel from '../../components/platform/vmdetail/VmPerfPanel'
import VmFixItList from '../../components/platform/fleet/VmFixItList'
import BootDoctorCard from '../../components/platform/vmdetail/BootDoctorCard'
import BackupHealthChip from '../../components/platform/BackupHealthChip'
import GuestDriftCard from '../../components/platform/vmdetail/GuestDriftCard'
import GuestAgentSetupDialog from '../../components/platform/fleet/GuestAgentSetupDialog'
import { vmHealthScore } from '../../utils/vmHealthScore'
import { useVmMetricSeries } from '../../hooks/useVmMetricSeries'
import JsonInspector from '../../components/platform/JsonInspector'
import { StructuredErrorBanner } from '../../components/StructuredErrorBanner'
import {
  createVmBackup,
  createVmSnapshot,
  type CreateVmSnapshotBody,
  deleteVmSnapshot,
  getPlatformVm,
  getPlatformVmSpec,
  getVmDisks,
  getPlatformVmMetrics,
  getVmMigrations,
  runVmHealthCheck,
  attachVmDisk,
  attachVmNic,
  detachVmDisk,
  detachVmNic,
  getVmLibvirtDetails,
  getVmPendingConfig,
  renamePlatformVm,
  injectVmNmi,
  type VmPendingConfig,
  listPlatformNetworks,
  resizeVmDisk,
  setVmAutostart,
  type VmLibvirtDetails,
  adoptPlatformVm,
  getVmHaPolicy,
  listPlatformHosts,
  listVmBackups,
  listVmSnapshots,
  listVmTimeline,
  migratePrecheck,
  patchVm,
  restoreVmBackup,
  verifyVmBackup,
  revertVmSnapshot,
  cloneVmSnapshot,
  getVmDomainXml,
  setVmHa,
  vmClone,
  installGuestTools,
  vmDelete,
  vmMigrate,
  vmPower,
  installPlatformVm,
  type PlatformHost,
  type PlatformVm,
  type MigratePrecheckResult,
  type HaPolicy,
  type SnapshotRecord,
  type VmTimelineEntry,
  type VmDiskRow,
  type VmMigrationRecord,
  type BackupRecord,
  type VmHealthReport,
  getVmGuestHealth,
  getVmGuestServices,
  guestServiceAction,
  diagnoseVm,
  getVmTopology,
  getConsoleHubPlan,
  listVmPortForwards,
  type VmPortForwardRule,
  type ConsoleHubPlan,
  type TopologyGraph,
  type VmGuestHealthReport,
  type GuestAiInsightsReport,
  type VmGuestServicesReport,
  type VmOsDiagnoseReport,
  platformVmViewerVvUrl,
} from '../../api/platform'
import { getVmDoctor, type VmDoctorReport } from '../../api/ai'
import { getVmGuestFirewallPorts, type GuestPortReport } from '../../api/zeusFirewall'
import { useAi } from '../../contexts/AiContext'
import { useToastContext } from '../../contexts/ToastContext'
import { formatUserError, isPlatformNotFoundError } from '../../utils/apiError'
import { formatPlatformHostLabel } from '../../utils/fleetDisplayName'
import { GUEST_TOAST_CHANNEL_ATTACH, qgaHealthy } from '../../utils/guestAgentUx'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import { purgeVmShortcuts } from '../../utils/vmShortcuts'
import VmStatusBadge from '../../components/VmStatusBadge'
import {hostStateTone, httpStatusTone, migrationReadinessTone, riskTone, statusBadgeClasses, statusPillClasses, statusSurfaceClasses, statusToneClass, taskStatusTone, webhookDeliveryTone, hubLinkClasses} from '../../utils/semanticColors'
import { cinemaHubPath, studioHubPath } from '../../utils/consoleExperienceMode'
import { vmErrorPresentation } from '../../utils/vmErrorPresentation'
import { loadVmSshPrefs } from '../../utils/vmSshPrefs'
import VmConnectHub from '../../components/vm/VmConnectHub'
import VmConsoleHeroPreview from '../../components/vm/VmConsoleHeroPreview'
import VmDetailActionBar from '../../components/platform/VmDetailActionBar'
import VmAttentionStack from '../../components/platform/VmAttentionStack'
import VmDetailHero from '../../components/platform/VmDetailHero'
import VmPortForwardPanel from '../../components/vm/VmPortForwardPanel'
import VmSshConnectDialog, { navigateVmSshSession } from '../../components/vm/VmSshConnectDialog'
import { isCenterPopoutMode, openCenterPopout } from '../../utils/platformCenterPopout'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import { tasksHubHref } from '../../utils/platformHubLinks'
import { downloadVmIacBundle, downloadVmIacZip, exportVmDisk, exportVmIac, pruneStaleVmRecord, retirePlatformVm, type VmIacExportBundle } from '../../api/platformVmLifecycle'
import { publishVmAsTemplate } from '../../api/platformTemplatesExtra'
import PlatformVmAdvanced from '../../components/platform/PlatformVmAdvanced'
import VmDevicesPanel from '../../components/platform/VmDevicesPanel'
import VmGraphicsPanel from '../../components/platform/VmGraphicsPanel'
import VmQemuLogsPanel from '../../components/platform/VmQemuLogsPanel'
import VmPendingBadge from '../../components/platform/VmPendingBadge'
import SpotlightPageAction from '../../components/platform/SpotlightPageAction'
import { invokeVmLibvirt, queryVmLibvirt, precheckVmSnapshot, precheckVmSnapshotAction, type CpuMemoryTopology, type SnapshotPrecheck } from '../../api/platformVmLibvirt'
import VmCpuTopologyModal from '../../components/platform/VmCpuTopologyModal'
import VmMemorySizingModal from '../../components/platform/VmMemorySizingModal'
import { putVmDomainXml } from '../../api/platformVmLibvirt'
import { buildVmSpotlightPrefill, vmDetailBlockers } from '../../utils/vmDetailSpotlight'
import { sshNatHostPort } from '../../utils/vmPortForwardServices'
import { formatBytes } from '../../utils/vm'
import { getSession, type SessionRole } from '../../api/auth'
import { listIsos, type ImageFile } from '../../api/extras'
import { BrowseHostPathModal, isHostDiskImageFileName, isIsoFileName } from '../../components/BrowseHostPathModal'
import { useVmHardware } from '../../hooks/useVmHardware'
import { useKubevirtHardware } from '../../hooks/useKubevirtHardware'
import VmHardwareDrawer from '../../components/vm/VmHardwareDrawer'
import VmKubevirtHardwareDrawer from '../../components/vm/VmKubevirtHardwareDrawer'
import VmHardwareSection from '../../components/vm/VmHardwareSection'
import VmWindowsReadinessPanel from '../../components/vm/VmWindowsReadinessPanel'
import VmEditHardwareDrawer from '../../components/vm/VmEditHardwareDrawer'
import { listVmSchedules, createVmSchedule, deleteVmSchedule, type VmSchedule, type VmScheduleAction } from '../../api/platformVmSchedules'
import { useBreadcrumbName } from '../../contexts/BreadcrumbNameContext'

export default function PlatformVmDetail() {
  const location = useLocation()
  const navigate = useNavigate()
  const isPopout = isCenterPopoutMode(location.search)
  const { id } = useParams<{ id: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const tabParam = searchParams.get('tab')
  const rawTab = tabParam === 'guestPorts' ? 'security' : tabParam
  const tab: VmDetailTab = (
    ['overview', 'access', 'hardware', 'doctor', 'console', 'performance', 'disks', 'devices', 'network', 'guestHealth', 'guestServices', 'security', 'snapshots', 'backup', 'topology', 'events', 'logs', 'settings', 'advanced'] as VmDetailTab[]
  ).includes(rawTab as VmDetailTab) ? (rawTab as VmDetailTab) : 'overview'
  const setTab = (next: VmDetailTab, extra?: { guestAction?: string }) => {
    if (next === 'console' && id) {
      navigate(cinemaHubPath(id))
      return
    }
    setSearchParams((p) => {
      const n = new URLSearchParams(p)
      if (next === 'overview') n.delete('tab')
      else n.set('tab', next)
      if (extra?.guestAction) n.set('guestAction', extra.guestAction)
      else n.delete('guestAction')
      return n
    }, { replace: true })
  }
  const guestMigratePlanAction = searchParams.get('guestAction') === 'migrate-plan'
  const deepAction = searchParams.get('action')
  const [agentSetupOpen, setAgentSetupOpen] = useState(deepAction === 'agent')
  const [bootDoctorEngaged, setBootDoctorEngaged] = useState(false)
  const [tier] = usePlatformDesktopTier()
  const { info } = usePlatformInfo()
  const { setContextVmId, setContextSummary, openCopilot } = useAi()
  const toast = useToastContext()
  const [vm, setVm] = useState<PlatformVm | null>(null)
  useBreadcrumbName(vm?.name ?? '')
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [error, setError] = useState<string | null>(null)
  const [destHost, setDestHost] = useState('')
  const [migrateLive, setMigrateLive] = useState(true)
  const [migrateBandwidth, setMigrateBandwidth] = useState('')
  const [migratePostcopy, setMigratePostcopy] = useState(false)
  const [migrateUndefineSource, setMigrateUndefineSource] = useState(false)
  const [migrateTunnelled, setMigrateTunnelled] = useState(false)
  const [migrateDisks, setMigrateDisks] = useState('')
  const [migrateDisksUri, setMigrateDisksUri] = useState('')
  const [migrateCopyStorage, setMigrateCopyStorage] = useState(false)
  const [timeline, setTimeline] = useState<VmTimelineEntry[]>([])
  const [cloneName, setCloneName] = useState('')
  const [cloneMode, setCloneMode] = useState<'linked' | 'full'>('linked')
  const [precheck, setPrecheck] = useState<MigratePrecheckResult | null>(null)
  const [ha, setHa] = useState<HaPolicy>({ enabled: false, restart_attempts: 3, restart_priority: 'medium', fence_on_failure: false, anti_affinity: false })
  const [specJson, setSpecJson] = useState<string>('')
  const [specData, setSpecData] = useState<Record<string, unknown> | null>(null)
  const [snapName, setSnapName] = useState('')
  const [snapDiskOnly, setSnapDiskOnly] = useState(true)
  const [snapQuiesce, setSnapQuiesce] = useState(false)
  const [snapStorageMode, setSnapStorageMode] = useState('')
  const [snapPrecheck, setSnapPrecheck] = useState<SnapshotPrecheck | null>(null)
  const [snapPrecheckLoading, setSnapPrecheckLoading] = useState(false)
  const [cpuModalOpen, setCpuModalOpen] = useState(false)
  const [memoryModalOpen, setMemoryModalOpen] = useState(false)
  const [hardwareDrawerOpen, setHardwareDrawerOpen] = useState(false)
  const [hardwareEditOpen, setHardwareEditOpen] = useState(false)
  const [computeTopology, setComputeTopology] = useState<CpuMemoryTopology | null>(null)
  const [computeTopologyLoading, setComputeTopologyLoading] = useState(false)
  const [domainXmlSaving, setDomainXmlSaving] = useState(false)
  const [snapAiHint, setSnapAiHint] = useState<GuestAiInsightsReport | null>(null)
  const [snapAiLoading, setSnapAiLoading] = useState(false)
  const [publishTplName, setPublishTplName] = useState('')
  const [publishTplVersion, setPublishTplVersion] = useState('1.0.0')
  const [iacBundle, setIacBundle] = useState<VmIacExportBundle | null>(null)
  const [project, setProject] = useState('')
  const [tags, setTags] = useState('')
  const [snapshots, setSnapshots] = useState<SnapshotRecord[]>([])
  const [backups, setBackups] = useState<BackupRecord[]>([])
  const [disks, setDisks] = useState<VmDiskRow[]>([])
  const [metrics, setMetrics] = useState<{ cpu_percent: number; memory_used_mib: number; updated_at: string } | null>(null)
  const [attachPath, setAttachPath] = useState('/var/lib/libvirt/images/data.qcow2')
  const [attachDev, setAttachDev] = useState('vdb')
  const [isoPath, setIsoPath] = useState('/var/lib/libvirt/images/debian-12.iso')
  const [isoTarget, setIsoTarget] = useState('')
  const [isoFiles, setIsoFiles] = useState<ImageFile[]>([])
  const [isoBrowseOpen, setIsoBrowseOpen] = useState(false)
  const [attachDiskBrowseOpen, setAttachDiskBrowseOpen] = useState(false)
  const [sessionRole, setSessionRole] = useState<SessionRole | null>(null)
  const [libvirtDetails, setLibvirtDetails] = useState<VmLibvirtDetails | null>(null)
  const [libvirtDetailsLoading, setLibvirtDetailsLoading] = useState(false)
  const [pendingConfig, setPendingConfig] = useState<VmPendingConfig | null>(null)
  const [pendingConfigLoading, setPendingConfigLoading] = useState(false)
  const [domainXml, setDomainXml] = useState('')
  const [schedules, setSchedules] = useState<VmSchedule[]>([])
  const [schedulesLoading, setSchedulesLoading] = useState(false)
  const [newSchedAction, setNewSchedAction] = useState<VmScheduleAction>('snapshot')
  const [newSchedInterval, setNewSchedInterval] = useState(1440)
  const [newSchedRetention, setNewSchedRetention] = useState<number | null>(5)
  const [newSchedLabel, setNewSchedLabel] = useState('')
  const [newSchedSaving, setNewSchedSaving] = useState(false)
  const [renameDraft, setRenameDraft] = useState('')
  const [descriptionDraft, setDescriptionDraft] = useState('')
  const [resizeTarget, setResizeTarget] = useState('')
  const [resizeGb, setResizeGb] = useState('10')
  const [nicNetwork, setNicNetwork] = useState('default')
  const [nicModel, setNicModel] = useState('virtio')
  const [diskEditTarget, setDiskEditTarget] = useState<string | null>(null)
  const [diskEditCache, setDiskEditCache] = useState('none')
  const [diskEditBus, setDiskEditBus] = useState('virtio')
  const [diskEditReadonly, setDiskEditReadonly] = useState(false)
  const [nicEditMac, setNicEditMac] = useState<string | null>(null)
  const [nicEditModel, setNicEditModel] = useState('virtio')
  const [nicEditNetwork, setNicEditNetwork] = useState('default')
  const [platformNetworks, setPlatformNetworks] = useState<Array<{ name: string }>>([])
  const [health, setHealth] = useState<VmHealthReport | null>(null)
  const [doctor, setDoctor] = useState<VmDoctorReport | null>(null)
  const [healthLoading, setHealthLoading] = useState(false)
  const [doctorLoading, setDoctorLoading] = useState(false)
  const [guestInstalling, setGuestInstalling] = useState(false)
  const [guestPorts, setGuestPorts] = useState<GuestPortReport | null>(null)
  const [guestPortsLoading, setGuestPortsLoading] = useState(false)
  const [guestHealth, setGuestHealth] = useState<VmGuestHealthReport | null>(null)
  // Start true so first Guest health paint shows "Testing…" instead of an empty placeholder.
  const [guestHealthLoading, setGuestHealthLoading] = useState(true)
  const [guestHealthError, setGuestHealthError] = useState<string | null>(null)
  const [guestHealthRefreshedAt, setGuestHealthRefreshedAt] = useState<Date | null>(null)
  const [guestServices, setGuestServices] = useState<VmGuestServicesReport | null>(null)
  const [guestServicesError, setGuestServicesError] = useState<string | null>(null)
  const [topology, setTopology] = useState<TopologyGraph | null>(null)
  const [guestServicesLoading, setGuestServicesLoading] = useState(false)
  const [guestServiceBusy, setGuestServiceBusy] = useState<string | null>(null)
  const [vmDiagnose, setVmDiagnose] = useState<VmOsDiagnoseReport | null>(null)
  const [vmDiagnoseLoading, setVmDiagnoseLoading] = useState(false)
  const [migrations, setMigrations] = useState<VmMigrationRecord[]>([])
  const [sshDialogOpen, setSshDialogOpen] = useState(false)
  const [portForwardRules, setPortForwardRules] = useState<VmPortForwardRule[]>([])
  const [consolePlan, setConsolePlan] = useState<ConsoleHubPlan | null>(null)
  const [vmConfirmOp, setVmConfirmOp] = useState<'delete' | 'delete_kubevirt' | 'remove_stale' | 'retire' | null>(null)
  const [snapConfirmMsg, setSnapConfirmMsg] = useState<{ message: string; action: string } | null>(null)
  const snapActionRef = useRef<(() => Promise<void>) | null>(null)
  const [snapDestructiveConfirm, setSnapDestructiveConfirm] = useState<{ snapName: string; action: 'delete' | 'revert'; run: () => Promise<unknown>; label: string } | null>(null)

  const migrationDisks = useMemo(
    () => (libvirtDetails?.disks ?? []).filter((d) => d.device === 'disk' && d.source),
    [libvirtDetails],
  )

  // The CD-ROM target <select> below lists real cdrom disks once libvirtDetails loads (falling back
  // to sda/sdb/hda when none are reported), but a browser auto-selects the first <option> visually
  // even though the controlled `isoTarget` state stays '' until something calls setIsoTarget — so
  // "Insert ISO" could silently submit an empty target. Keep it synced to a real option.
  const cdromTargets = useMemo(() => {
    const real = (libvirtDetails?.disks ?? []).filter((d) => d.device === 'cdrom').map((d) => d.target)
    return real.length > 0 ? real : ['sda', 'sdb', 'hda']
  }, [libvirtDetails])
  useEffect(() => {
    if (!cdromTargets.includes(isoTarget)) setIsoTarget(cdromTargets[0] ?? '')
  }, [cdromTargets, isoTarget])

  const canBrowseHost = sessionRole === 'admin'

  const loadSeq = useRef(0)
  // Guards the one-time auto-pick of a migration destination. Keeping destHost
  // out of load's deps stops setDestHost() from recreating load and refetching
  // every endpoint a second time on mount.
  const destHostAutoSetRef = useRef(false)

  const load = useCallback(async () => {
    if (!id) return
    // Last-response-wins: rapid navigation between VMs can leave stale awaits in
    // flight; only the newest load may commit state so VM A can't overwrite VM B.
    const seq = ++loadSeq.current
    const alive = () => seq === loadSeq.current
    setError(null)
    try {
      // Resolve the inventory source first. Starting libvirt-only requests while
      // the VM is still unknown makes KubeVirt details emit a burst of expected
      // 400/404/500 responses before the source gate can take effect.
      const v = await getPlatformVm(id)
      if (!alive()) return
      const [h, policy, spec, snaps, bks, tline, dsk, mtr] = await Promise.all([
        listPlatformHosts().catch(() => [] as PlatformHost[]),
        getVmHaPolicy(id).catch(() => ({ enabled: false, restart_attempts: 3, restart_priority: 'medium', fence_on_failure: false, anti_affinity: false }) as HaPolicy),
        getPlatformVmSpec(id).catch(() => null),
        listVmSnapshots(id).catch(() => [] as SnapshotRecord[]),
        listVmBackups(id).catch(() => [] as BackupRecord[]),
        listVmTimeline(id).catch(() => [] as VmTimelineEntry[]),
        getVmDisks(id).catch(() => [] as VmDiskRow[]),
        v.inventory_source === 'kubevirt' ? Promise.resolve(null) : getPlatformVmMetrics(id).catch(() => null),
      ])
      if (!alive()) return
      setVm(v)
      setHosts(h)
      setHa(policy)
      setSpecJson(JSON.stringify(spec, null, 2))
      setSpecData(spec as Record<string, unknown>)
      const labels = (spec as { metadata?: { labels?: Record<string, string> } })?.metadata?.labels
      setDescriptionDraft(labels?.description?.trim() ?? '')
      setProject(v.project || '')
      setTags((v.tags || []).join(', '))
      setSnapshots(snaps)
      setBackups(bks)
      setTimeline(tline)
      setDisks(dsk)
      setMetrics(mtr)
      if (!destHostAutoSetRef.current && h.length > 1) {
        destHostAutoSetRef.current = true
        setDestHost((prev) => prev || h.find((x) => x.id !== v.host_id)?.id || h[0]?.id || '')
      }
    } catch (e: unknown) {
      if (!alive()) return
      // Render the state inline (with Retry) instead of bouncing to the VM list.
      // The old redirect fired on a fuzzy "not found" substring match, so an
      // unrelated transient error could yank the user off the page entirely.
      setError(
        isPlatformNotFoundError(e)
          ? 'This virtual machine no longer exists — it may have been removed.'
          : formatUserError(e),
      )
    }
  }, [id])

  useEffect(() => {
    if (tab !== 'topology' || !id) return
    let cancelled = false
    void getVmTopology(id)
      .then((t) => { if (!cancelled) setTopology(t) })
      .catch(() => { if (!cancelled) setTopology(null) })
    return () => { cancelled = true }
  }, [tab, id])

  const runHealth = useCallback(async () => {
    if (!id) return
    setHealthLoading(true)
    try {
      setHealth(await runVmHealthCheck(id))
    } catch {
      setHealth(null)
    } finally {
      setHealthLoading(false)
    }
  }, [id])

  const runDoctor = useCallback(async () => {
    if (!id) return
    setDoctorLoading(true)
    try {
      setDoctor(await getVmDoctor(id))
    } catch {
      setDoctor(null)
    } finally {
      setDoctorLoading(false)
    }
  }, [id])

  useEffect(() => { void load() }, [load])

  // Reset per-VM state on navigation between VMs so the previous VM's details,
  // topology, and migration history don't flash under the new id. Clearing `vm`
  // re-triggers the contentLoading spinner (see contentLoading={!vm && !error}).
  useEffect(() => {
    setVm(null)
    setTopology(null)
    setMigrations([])
    setGuestHealth(null)
    setGuestHealthLoading(true)
    setGuestHealthError(null)
    // Let the next VM auto-pick its own migration destination.
    destHostAutoSetRef.current = false
  }, [id])

  useEffect(() => {
    getSession()
      .then((s) => {
        if (s.authenticated) setSessionRole(s.role ?? 'admin')
        else setSessionRole(null)
      })
      .catch(() => setSessionRole(null))
  }, [])

  useEffect(() => { void runHealth() }, [runHealth])
  useEffect(() => { void runDoctor() }, [runDoctor])

  const loadPortForwards = useCallback(async () => {
    if (!id) return
    try {
      const [rules, plan] = await Promise.all([
        listVmPortForwards(id),
        getConsoleHubPlan(id).catch(() => null),
      ])
      setPortForwardRules(rules)
      if (plan) setConsolePlan(plan)
    } catch {
      setPortForwardRules([])
    }
  }, [id])

  const loadConsolePlan = useCallback(async () => {
    if (!id) return
    try {
      setConsolePlan(await getConsoleHubPlan(id))
    } catch {
      setConsolePlan(null)
    }
  }, [id])

  useEffect(() => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') return
    void loadConsolePlan()
  }, [id, vm?.inventory_source, loadConsolePlan])

  useEffect(() => {
    const ip = guestHealth?.guest_ip?.trim() || health?.guest_ip?.trim() || vm?.guest_ip?.trim() || ''
    if (!id || !ip || vm?.inventory_source === 'kubevirt') {
      setPortForwardRules([])
      return
    }
    void loadPortForwards()
  }, [id, guestHealth?.guest_ip, health?.guest_ip, vm?.guest_ip, vm?.inventory_source, loadPortForwards])

  const hardware = useVmHardware({
    vmId: id,
    enabled: Boolean(id && vm && vm.inventory_source !== 'kubevirt'),
    inventorySource: vm?.inventory_source,
    portForwardRules,
    protocols: consolePlan?.protocols ?? [],
    osHint: consolePlan?.os_hint,
  })
  const kubevirtHardware = useKubevirtHardware({
    vmId: id,
    enabled: Boolean(id && vm?.inventory_source === 'kubevirt'),
  })

  useEffect(() => {
    if (!id || tab !== 'events') return
    let cancelled = false
    void getVmMigrations(id)
      .then((m) => { if (!cancelled) setMigrations(m) })
      .catch(() => { if (!cancelled) setMigrations([]) })
    return () => { cancelled = true }
  }, [id, tab])

  const loadGuestPorts = useCallback(async () => {
    if (!id) return
    setGuestPortsLoading(true)
    try {
      setGuestPorts(await getVmGuestFirewallPorts(id))
    } catch {
      setGuestPorts(null)
    } finally {
      setGuestPortsLoading(false)
    }
  }, [id])

  const loadGuestHealth = useCallback(async (opts?: { quiet?: boolean }) => {
    if (!id) return
    if (!opts?.quiet) setGuestHealthLoading(true)
    setGuestHealthError(null)
    try {
      const gh = await getVmGuestHealth(id)
      setGuestHealth(gh)
      setGuestHealthRefreshedAt(new Date())
      if (gh.os_pretty_name || gh.install_state === 'running') {
        const chip = [gh.os_pretty_name, qgaHealthy(gh) ? 'QGA' : gh.install_state]
          .filter(Boolean)
          .join(' · ')
        setContextSummary(chip || null)
      }
    } catch (e: unknown) {
      if (!opts?.quiet) setGuestHealth(null)
      setGuestHealthError(formatUserError(e))
    } finally {
      setGuestHealthLoading(false)
    }
  }, [id])

  const loadGuestServices = useCallback(async (opts?: { quiet?: boolean }) => {
    if (!id) return
    // Keep existing rows visible while refreshing after start/stop (avoids "Loading…" flash in UX/demo).
    if (!opts?.quiet) setGuestServicesLoading(true)
    setGuestServicesError(null)
    try {
      setGuestServices(await getVmGuestServices(id))
    } catch (e: unknown) {
      setGuestServices(null)
      setGuestServicesError(formatUserError(e))
    } finally {
      setGuestServicesLoading(false)
    }
  }, [id])

  const runGuestServiceAction = useCallback(
    async (unit: string, action: 'start' | 'stop' | 'restart') => {
      if (!id) return
      const key = `${unit}:${action}`
      setGuestServiceBusy(key)
      try {
        const r = await guestServiceAction(id, unit, action)
        toast.success(r.message || `${action} ${unit}`)
      } catch (e: unknown) {
        toast.error(formatUserError(e))
      } finally {
        setGuestServiceBusy(null)
      }
      void loadGuestServices({ quiet: true })
    },
    [id, loadGuestServices, toast],
  )

  const queueGuestToolsInstall = useCallback(async () => {
    if (!id) return
    setGuestInstalling(true)
    try {
      const r = await installGuestTools(id)
      toastQueuedOperation(toast, GUEST_TOAST_CHANNEL_ATTACH, r.task_id, tier)
      await loadGuestHealth()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setGuestInstalling(false)
    }
  }, [id, loadGuestHealth, tier, toast])

  const runGuestAction: RunGuestActionFn = useCallback(
    async (key, fn, success) => {
      try {
        await fn()
        toast.success(success)
        await loadGuestHealth({ quiet: true })
      } catch (e: unknown) {
        toast.error(formatUserError(e))
      }
    },
    [loadGuestHealth, toast],
  )

  const runVmDiagnose = useCallback(async (query?: string) => {
    if (!id) return
    setVmDiagnoseLoading(true)
    try {
      setVmDiagnose(await diagnoseVm(id, query))
    } catch {
      setVmDiagnose(null)
    } finally {
      setVmDiagnoseLoading(false)
    }
  }, [id])

  useEffect(() => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') {
      if (vm?.inventory_source === 'kubevirt') {
        setGuestHealthLoading(false)
        setGuestServicesLoading(false)
      }
      return
    }
    void loadGuestHealth()
    // Prefetch services so the Guest services tab is warm on first open.
    void loadGuestServices({ quiet: true })
  }, [id, vm?.inventory_source, loadGuestHealth, loadGuestServices])

  useEffect(() => {
    if (!id || !vm || vm.observed_state !== 'running' || vm.inventory_source === 'kubevirt') return
    void loadGuestPorts()
  }, [id, vm?.observed_state, vm?.inventory_source, loadGuestPorts])

  const loadComputeTopology = useCallback(async () => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') return
    setComputeTopologyLoading(true)
    try {
      setComputeTopology(await queryVmLibvirt<CpuMemoryTopology>(id, 'cpu.memory.topology'))
    } catch {
      setComputeTopology(null)
    } finally {
      setComputeTopologyLoading(false)
    }
  }, [id, vm?.inventory_source])

  const loadLibvirtDetails = useCallback(async () => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') return
    setLibvirtDetailsLoading(true)
    try {
      const details = await getVmLibvirtDetails(id)
      setLibvirtDetails(details)
    } catch {
      setLibvirtDetails(null)
    } finally {
      setLibvirtDetailsLoading(false)
    }
  }, [id, vm?.inventory_source])

  const loadPendingConfig = useCallback(async () => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') return
    if (vm?.observed_state !== 'running' && vm?.observed_state !== 'paused') {
      setPendingConfig(null)
      return
    }
    setPendingConfigLoading(true)
    try {
      setPendingConfig(await getVmPendingConfig(id))
    } catch {
      setPendingConfig(null)
    } finally {
      setPendingConfigLoading(false)
    }
  }, [id, vm?.inventory_source, vm?.observed_state])

  const loadDomainXml = useCallback(async () => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') return
    try {
      const { xml } = await getVmDomainXml(id)
      setDomainXml(xml)
    } catch {
      setDomainXml('')
    }
  }, [id, vm?.inventory_source])

  const loadSchedules = useCallback(async () => {
    if (!id) return
    setSchedulesLoading(true)
    try { setSchedules(await listVmSchedules(id)) } catch (e: unknown) { setSchedules([]); toast.error(formatUserError(e)) } finally { setSchedulesLoading(false) }
  }, [id])

  useEffect(() => {
    if (id && vm && vm.inventory_source !== 'kubevirt') void loadPendingConfig()
  }, [id, vm?.inventory_source, vm?.observed_state, loadPendingConfig])

  useEffect(() => {
    if (!id || !vm || vm.inventory_source === 'kubevirt') return
    if (tab === 'security' || tab === 'access' || tab === 'overview') void loadGuestPorts()
    if (tab === 'guestServices') void loadGuestServices()
    if (tab === 'overview' || tab === 'access' || tab === 'hardware' || tab === 'disks' || tab === 'devices' || tab === 'network' || tab === 'settings' || tab === 'advanced') {
      if (tab === 'overview') void loadComputeTopology()
      if (tab === 'overview' || tab === 'access' || tab === 'settings' || tab === 'advanced' || tab === 'devices') void loadDomainXml()
      if (tab === 'settings') void loadSchedules()
      void loadLibvirtDetails()
      if (tab === 'disks') {
        void listIsos()
          .then((r) => setIsoFiles(r.files ?? []))
          .catch(() => setIsoFiles([]))
      }
      if (tab === 'network') {
        void listPlatformNetworks()
          .then((nets) => setPlatformNetworks(nets.map((n) => ({ name: n.name }))))
          .catch(() => setPlatformNetworks([]))
      }
    }
  }, [tab, id, loadComputeTopology, loadGuestPorts, loadGuestServices, loadLibvirtDetails, loadDomainXml, loadSchedules, vm?.inventory_source])

  useEffect(() => {
    if (tab !== 'snapshots' || !id || !vm || vm.inventory_source === 'kubevirt') {
      setSnapPrecheck(null)
      return
    }
    let ignore = false
    setSnapPrecheckLoading(true)
    // Debounced so precheck isn't refired on every keystroke of the name field,
    // and `ignore` drops the response of a request superseded by a newer one.
    const timer = setTimeout(() => {
      void precheckVmSnapshot(id, {
        name: snapName.trim() || 'snap-01',
        disk_only: snapDiskOnly,
        quiesce: snapQuiesce,
        storage_mode: snapStorageMode || undefined,
      })
        .then((r) => {
          if (!ignore) setSnapPrecheck(r)
        })
        .catch(() => {
          if (!ignore) setSnapPrecheck(null)
        })
        .finally(() => {
          if (!ignore) setSnapPrecheckLoading(false)
        })
    }, 400)
    return () => {
      ignore = true
      clearTimeout(timer)
    }
  }, [tab, id, vm?.inventory_source, snapName, snapDiskOnly, snapQuiesce, snapStorageMode])

  useEffect(() => {
    setContextVmId(id ?? null)
    return () => setContextVmId(null)
  }, [id, setContextVmId])

  const act = async (label: string, fn: () => Promise<unknown>) => {
    try {
      const r = await fn()
      if (r && typeof r === 'object' && 'task_id' in r && typeof (r as { task_id: string }).task_id === 'string') {
        toastQueuedOperation(toast, label, (r as { task_id: string }).task_id, tier)
      } else {
        toast.success(label)
      }
      await load()
      if (tab === 'overview') await loadComputeTopology()
      if (tab === 'disks' || tab === 'devices' || tab === 'network' || tab === 'settings' || tab === 'advanced' || tab === 'overview') await loadLibvirtDetails()
      if (vm?.observed_state === 'running' || vm?.observed_state === 'paused') await loadPendingConfig()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runSnapshotAction = async (
    snapName: string,
    action: 'delete' | 'revert' | 'clone',
    run: () => Promise<unknown>,
    label: string,
  ) => {
    if (!id) return
    try {
      const pre = await precheckVmSnapshotAction(id, snapName, action)
      if (pre.blocked) {
        toast.error(pre.message || `${action} blocked`)
        return
      }
      if (pre.message) {
        snapActionRef.current = async () => act(label, run)
        setSnapConfirmMsg({ message: pre.message, action })
        return
      }
      await act(label, run)
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const queueVmDelete = async (label: string) => {
    if (!id) return
    try {
      if (vm?.observed_state === 'missing') {
        const r = await pruneStaleVmRecord(id)
        if (r.name) purgeVmShortcuts([r.name])
        toast.success(label)
        navigate('/platform/vms', { replace: true })
        return
      }
      const r = await vmDelete(id, true)
      if (vm?.name) purgeVmShortcuts([vm.name])
      if (r.status === 'completed') {
        toast.success(label)
        navigate('/platform/vms', { replace: true })
        return
      }
      navigate('/platform/vms', { replace: true, state: { vmDeleteTaskId: r.task_id, vmDeleteLabel: label } })
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const hostRow = hosts.find((h) => h.id === vm?.host_id)
  const hostLabel = hostRow
    ? formatPlatformHostLabel(hostRow, {
        fallbackAddress: typeof window !== 'undefined' ? window.location.hostname : undefined,
      })
    : 'No host'

  const resolvedGuestIp =
    guestHealth?.guest_ip?.trim() || health?.guest_ip?.trim() || vm?.guest_ip?.trim() || ''

  useEffect(() => {
    if (!id || !vm || vm.observed_state !== 'running' || resolvedGuestIp || vm.inventory_source === 'kubevirt') {
      return
    }
    let cancelled = false
    let attempts = 0
    const poll = () => {
      if (cancelled || attempts >= 24) return
      attempts += 1
      void getVmGuestHealth(id)
        .then((gh) => {
          if (gh?.guest_ip?.trim()) setGuestHealth(gh)
        })
        .catch(() => undefined)
    }
    poll()
    const t = setInterval(poll, 5000)
    return () => {
      cancelled = true
      clearInterval(t)
    }
  }, [id, vm?.observed_state, vm?.inventory_source, resolvedGuestIp])

  const metricSeries = useVmMetricSeries(id, vm?.observed_state === 'running' && vm?.inventory_source !== 'kubevirt', metrics)

  if (!id) return null

  const sshUser = (() => {
    const ci = specData?.cloud_init as { user?: string } | undefined
    return ci?.user?.trim() || loadVmSshPrefs(vm?.name ?? '')?.user || 'root'
  })()
  const guestIp = resolvedGuestIp
  const hypervisorAddress = hostRow?.address?.trim() || undefined
  const guestAccess = consolePlan?.guest_access ?? null

  const natForwardHref = guestIp
    ? `/host-networking?tab=portforward&vm_ip=${encodeURIComponent(guestIp)}&vm_port=22`
    : undefined

  const canInstall = Boolean(
    vm?.tags?.includes('define-only')
    && ['stopped', 'shut off', 'shutoff'].includes(vm?.observed_state ?? ''),
  )

  const detailBlockers = vm
    ? vmDetailBlockers({
        pending: pendingConfig,
        guestHealth,
        guestToolsStatus: health?.guest_tools_status,
        observedState: vm.observed_state,
        guestIp,
        guestAccess,
        portForwardRules,
      })
    : []

  const spotlightPrefill = vm
    ? buildVmSpotlightPrefill({
        vmName: vm.name,
        observedState: vm.observed_state,
        guestIp,
        healthScore: vmHealthScore(health),
        doctor,
        blockers: detailBlockers,
      })
    : ''

  const connectHubProps = vm && vm.inventory_source !== 'kubevirt' ? {
    vmName: vm.name,
    vmState: vm.observed_state,
    sshUser,
    guestIp,
    consoleHref: cinemaHubPath(id!),
    specJson,
    platformVmId: id,
    hypervisorAddress,
    guestAccess,
    portForwardRules,
    onRefreshPortForwards: () => void loadPortForwards(),
    guestIpWaiting: vm.observed_state === 'running' && !guestIp,
    guestIpHint: guestHealth?.issues?.[0],
    onRefreshGuestIp: () => void loadGuestHealth(),
    onInstallGuestTools: vm.observed_state === 'running' ? () => void queueGuestToolsInstall() : undefined,
    guestToolsInstalling: guestInstalling,
    onExportXml: async () => {
      const { xml } = await getVmDomainXml(id!)
      return xml
    },
    guestPorts,
    guestPortsLoading,
    onRefreshPorts: () => void loadGuestPorts(),
    onAllPorts: () => setTab('security'),
    natForwardHref,
    onNotify: (m: string) => toast.success(m),
    onOpenAccessTab: () => setTab('access'),
  } : null

  return (
    <PageLayout
      compact
      prepend={!isPopout ? (
        <Link to="/platform/vms" className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
          <ArrowLeft className="w-4 h-4" /> Virtual Machines
        </Link>
      ) : undefined}
      title={vm?.name ?? 'Virtual machine'}
      subtitle={vm ? (
        <span className="flex flex-wrap items-center gap-2 text-sm">
          <VmStatusBadge state={vm.observed_state} />
          {vm.lifecycle_phase && vm.lifecycle_phase !== vm.observed_state && (
            <VmStatusBadge state={vm.lifecycle_phase} />
          )}
          <span className="text-[var(--text-muted)] max-sm:hidden" aria-hidden>·</span>
          <span className="text-[var(--text-muted)]" title={hostRow?.address ?? undefined}>{hostLabel}</span>
          {guestIp && (
            <>
              <span className="text-[var(--text-muted)] max-sm:hidden" aria-hidden>·</span>
              <span className="font-mono text-emerald-600/90">{guestIp}</span>
            </>
          )}
          <GuestAgentHeaderPill report={guestHealth} onClick={() => setTab('guestHealth')} />
          {guestHealth && !qgaHealthy(guestHealth) && vm.observed_state === 'running' && (
            <button
              type="button"
              className={statusPillClasses(installStateTone(guestHealth.install_state, guestHealth.agent_ping))}
              onClick={() => setTab('guestHealth')}
            >
              Guest agent
            </button>
          )}
          {vm.ha_enabled && <span className={statusPillClasses('info')}>HA</span>}
        </span>
      ) : undefined}
      icon={<Monitor className="w-6 h-6 text-[var(--text-muted)]" />}
      actions={vm && id ? (
        vm.inventory_source === 'kubevirt' ? (
          <div className="flex flex-wrap items-center gap-2">
            <Link to={cinemaHubPath(id)} className="btn-primary text-sm inline-flex items-center gap-1">
              <Monitor className="w-4 h-4" /> Open Cinema
            </Link>
            <button type="button" className="btn-secondary text-sm inline-flex items-center gap-1" onClick={() => setHardwareDrawerOpen(true)} data-testid="vm-detail-hardware">
              <Cpu className="w-4 h-4" /> Hardware
            </button>
            {!isPopout ? <SpotlightPageAction prefill={spotlightPrefill} label="Ask Zyra" /> : null}
          </div>
        ) : (
          <VmDetailActionBar
            vmId={id}
            vmName={vm.name}
            inventorySource={vm.inventory_source}
            observedState={vm.observed_state}
            canInstall={canInstall}
            guestHealth={guestHealth}
            isPopout={isPopout}
            virtViewerUrl={vm.observed_state === 'running' ? platformVmViewerVvUrl(id) : null}
            spotlightPrefill={spotlightPrefill}
            onSsh={() => setSshDialogOpen(true)}
            onOpenHardware={() => setHardwareDrawerOpen(true)}
            onDelete={() => setVmConfirmOp('delete')}
            onPopout={!isPopout ? () => openCenterPopout(`/platform/vms/${id}`) : undefined}
            act={act}
            power={{
              onInstall: canInstall ? () => void act('Install queued', () => installPlatformVm(id)) : undefined,
              onStart: (vm.observed_state === 'stopped' || vm.observed_state === 'shut off' || vm.observed_state === 'shutoff') && !canInstall
                ? () => void act('Start queued', () => vmPower(id, 'start'))
                : undefined,
              onResume: vm.observed_state === 'paused'
                ? () => void act('Resume queued', () => vmPower(id, 'resume'))
                : undefined,
              onShutdown: vm.observed_state === 'running'
                ? () => void act('Shutdown queued', () => vmPower(id, 'shutdown'))
                : undefined,
              onGracefulShutdown:
                vm.observed_state === 'running' && guestHealth?.install_state === 'running' && guestHealth.agent_ping
                  ? () => void act('Graceful shutdown queued', () => vmPower(id, 'shutdown', { mode: 'agent' }))
                  : undefined,
              onPause: vm.observed_state === 'running'
                ? () => void act('Pause queued', () => vmPower(id, 'pause'))
                : undefined,
              onReboot: vm.observed_state === 'running'
                ? () => void act('Reboot queued', () => vmPower(id, 'reboot'))
                : undefined,
              onGracefulReboot:
                vm.observed_state === 'running' && guestHealth?.install_state === 'running' && guestHealth.agent_ping
                  ? () => void act('Graceful reboot queued', () => vmPower(id, 'reboot', { mode: 'agent' }))
                  : undefined,
              onForceReboot: vm.observed_state === 'running'
                ? () => void act('Force reboot queued', () => vmPower(id, 'reset'))
                : undefined,
              onNmi: vm.observed_state === 'running'
                ? () => void act('NMI injected', () => injectVmNmi(id))
                : undefined,
              onForceStop: (vm.observed_state === 'running' || vm.observed_state === 'paused')
                ? () => void act('Force stop queued', () => vmPower(id, 'stop'))
                : undefined,
            }}
          />
        )
      ) : undefined}
      error={error}
      onErrorRetry={() => void load()}
      contentLoading={!vm && !error}
    >
      {vm && (
        <>
          {vm.inventory_source === 'kubevirt' && (
            <MacGlassPanel title="KubeVirt guest">
              <p className="text-sm text-[var(--text-secondary)]">
                Namespace: <span className="font-mono text-[var(--link)]">{vm.k8s_namespace ?? 'default'}</span>
              </p>
              <div className="flex flex-wrap gap-2 mt-2">
                {vm.observed_state !== 'running' && (
                  <button
                    type="button"
                    className="btn-secondary text-xs inline-flex items-center gap-1"
                    onClick={() =>
                      void act('Start requested', () =>
                        postK8sKubevirtVmLifecycle(vm.k8s_namespace ?? 'default', vm.name, 'start'),
                      )
                    }
                  >
                    <Play className="w-3.5 h-3.5" /> Start
                  </button>
                )}
                {vm.observed_state === 'running' && (
                  <>
                    <button
                      type="button"
                      className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-[var(--surface-hover)]/80 text-[var(--text-primary)] border border-[var(--apple-hairline)] hover:bg-[var(--surface-hover)]"
                      onClick={() =>
                        void act('Stop requested', () =>
                          postK8sKubevirtVmLifecycle(vm.k8s_namespace ?? 'default', vm.name, 'stop'),
                        )
                      }
                    >
                      <Square className="w-3.5 h-3.5" /> Stop
                    </button>
                    <button
                      type="button"
                      className="inline-flex items-center gap-1 px-2 py-1 rounded-md text-xs bg-amber-500/15 text-amber-700 border border-amber-500/35 hover:bg-amber-500/25"
                      onClick={() =>
                        void act('Restart requested', () =>
                          postK8sKubevirtVmLifecycle(vm.k8s_namespace ?? 'default', vm.name, 'restart'),
                        )
                      }
                    >
                      <RotateCcw className="w-3.5 h-3.5" /> Restart
                    </button>
                  </>
                )}
                <button
                  type="button"
                  className="btn-destructive text-xs inline-flex items-center gap-1"
                  onClick={() => setVmConfirmOp('delete_kubevirt')}
                >
                  <Trash2 className="w-3.5 h-3.5" /> Delete CR
                </button>
                <Link to="/platform/settings?section=integrations" className={`text-sm ${hubLinkClasses()}`}>K8s Workloads →</Link>
              </div>
            </MacGlassPanel>
          )}

          {vm.observed_state === 'missing' && (
            <MacGlassPanel title="Missing from inventory">
              <p className={`text-sm ${statusToneClass('warn')}`}>
                This VM is no longer reported by the last inventory scan. Sync hosts or remove the stale record.
              </p>
            </MacGlassPanel>
          )}
          {vm.managed === false && (
            <MacGlassPanel title="Discovered VM">
              <div className="flex items-center justify-between gap-3">
                <p className={`text-sm ${statusToneClass('warn')}`}>
                  {vm.inventory_source === 'kubevirt'
                    ? 'Discovered KubeVirt VM — adopt to track it in platform inventory.'
                    : 'Discovered on a host — adopt to manage lifecycle from the platform.'}
                </p>
                <button type="button" className="btn-primary text-sm" onClick={() => void act('VM adopted', () => adoptPlatformVm(id))}>Adopt VM</button>
              </div>
            </MacGlassPanel>
          )}
          {vm.last_error && (
            <StructuredErrorBanner error={vmErrorPresentation(vm.last_error)} />
          )}
          {vm.inventory_source !== 'kubevirt' && (
            <VmAttentionStack
              vmId={id}
              pending={pendingConfig}
              pendingLoading={pendingConfigLoading}
              guestHealth={guestHealth}
              guestToolsStatus={health?.guest_tools_status}
              observedState={vm.observed_state}
              guestIp={guestIp}
              guestAccess={guestAccess}
              portForwardRules={portForwardRules}
              guestToolsInstalling={guestInstalling}
              onShutdownForPending={() => void act('Shutdown queued', () => vmPower(id, 'shutdown'))}
              onOpenGuestHealth={() => setTab('guestHealth')}
              onInstallGuestTools={vm.observed_state === 'running' ? () => void queueGuestToolsInstall() : undefined}
              onOpenAccess={() => setTab('access')}
            />
          )}

          {vm.inventory_source !== 'kubevirt' && (
            <VmDetailHero
              observedState={vm.observed_state}
              guestIp={guestIp}
              healthScore={vmHealthScore(health)}
              doctorScore={doctor?.score_numeric ?? null}
              sshExposed={Boolean(sshNatHostPort(portForwardRules))}
              blockers={detailBlockers}
              onOpenAccess={() => setTab('access')}
              onOpenDoctor={() => setTab('doctor')}
            />
          )}

          <VmDetailTabs active={tab} onChange={setTab} />

          {isGuestRelatedTab(tab) && tab !== 'overview' && tab !== 'access' && (
            <GuestToolsStrip
              vmId={id}
              compact
              guestHealth={guestHealth}
              guestToolsStatus={health?.guest_tools_status}
              guestIp={guestHealth?.guest_ip || health?.guest_ip}
              guestHostname={guestHealth?.guest_hostname || health?.guest_hostname}
              installing={guestInstalling}
              onOpenGuestHealth={() => setTab('guestHealth')}
              onInstall={
                vm.observed_state === 'running' ? () => void queueGuestToolsInstall() : undefined
              }
            />
          )}

          {(vm.observed_state === 'missing' || (vm.last_error && /nodomain|domain not found|no domain with matching name|domain_not_found|kubevirt_not_found/i.test(vm.last_error))) && (
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="btn-destructive text-sm"
                onClick={() => setVmConfirmOp('remove_stale')}
              >
                <Trash2 className="w-4 h-4" /> Remove stale record
              </button>
              <Link to="/platform/hosts" className="btn-secondary text-sm inline-flex items-center">Sync hosts →</Link>
            </div>
          )}

          {tab === 'overview' && vm && (
            <VmOverviewStats
              series={metricSeries}
              vcpus={vm.vcpus}
              memoryMib={vm.memory_mib}
              healthScore={vmHealthScore(health) ?? (doctor ? vmHealthScore({ score_numeric: Number(doctor.score_numeric) }) : null)}
              running={vm.observed_state === 'running'}
            />
          )}

          {vm && (
            <GuestAgentSetupDialog open={agentSetupOpen} onClose={() => setAgentSetupOpen(false)} vm={vm} />
          )}

          {tab === 'overview' && vm && vm.inventory_source !== 'kubevirt' && (vm.observed_state !== 'running' || Boolean(vm.last_error) || deepAction === 'bootdoctor' || bootDoctorEngaged) && (
            <BootDoctorCard vm={vm} onChanged={() => void load()} onEngaged={() => setBootDoctorEngaged(true)} />
          )}

          {tab === 'overview' && vm && vm.inventory_source !== 'kubevirt' && vm.observed_state !== 'running' && <GuestDriftCard vm={vm} />}

          {tab === 'overview' && vm && health && (health.issues?.length ?? 0) > 0 && vm.observed_state === 'running' && (
            <VmFixItList vm={vm} issues={health.issues} onDone={() => void load()} onOpenTab={(t) => setTab(t as typeof tab)} />
          )}

          {tab === 'overview' && id && vm && (
            <VmConsoleHeroPreview
              vmName={vm.name}
              vmState={vm.observed_state}
              consoleHref={cinemaHubPath(id)}
              platformVmId={id}
            />
          )}

          {tab === 'overview' && connectHubProps && (
            <VmConnectHub {...connectHubProps} natExpanded={false} />
          )}

          {tab === 'access' && connectHubProps && (
            <div className="space-y-4 pt-2">
              <VmConnectHub {...connectHubProps} natExpanded showExport />
              <MacGlassPanel title="Guest security & ports" subtitle="In-guest listeners and host firewall">
                {vm.host_id && (
                  <Link to={`/platform/zeus/security/firewall/${vm.host_id}`} className={`text-sm inline-flex items-center gap-1 mb-3 ${hubLinkClasses()}`}>
                    <Shield className="w-4 h-4" /> Host firewall (Zeus) →
                  </Link>
                )}
                <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => setTab('security')}>
                  Full port inventory →
                </button>
              </MacGlassPanel>
              <MacGlassPanel title="Guest health">
                <p className="text-sm text-[var(--text-muted)] mb-2">Agent status, offline assurance, and service inventory.</p>
                <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => setTab('guestHealth')}>
                  Open Guest health →
                </button>
              </MacGlassPanel>
            </div>
          )}

          {tab === 'hardware' && vm.inventory_source === 'kubevirt' && id && (
            <div className="space-y-4 pt-2" data-testid="vm-hardware-tab">
              <MacGlassPanel title="KubeVirt hardware" subtitle="Cluster-managed VirtualMachine template — edit CPU/memory when stopped">
                {kubevirtHardware.loading && !kubevirtHardware.summary ? (
                  <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
                ) : kubevirtHardware.summary ? (
                  <div className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] px-3 py-1">
                    <VmHardwareSection label="CPU" value={kubevirtHardware.summary.cpu} testId="vm-hardware-cpu" />
                    <VmHardwareSection label="Memory" value={kubevirtHardware.summary.memory} testId="vm-hardware-memory" />
                    <VmHardwareSection label="Firmware" value={kubevirtHardware.summary.firmware} testId="vm-hardware-firmware" />
                    <VmHardwareSection label="Display" value={kubevirtHardware.summary.display} testId="vm-hardware-display" />
                    <VmHardwareSection label="NIC" value={kubevirtHardware.summary.nic} testId="vm-hardware-nic" />
                    <VmHardwareSection label="Cluster" value={kubevirtHardware.summary.cluster} testId="vm-kubevirt-hardware-cluster" />
                    <VmHardwareSection label="Node" value={kubevirtHardware.summary.node} testId="vm-kubevirt-hardware-node" />
                  </div>
                ) : (
                  <p className="text-sm text-[var(--text-muted)]">Hardware details unavailable.</p>
                )}
                <div className="flex flex-wrap gap-2 mt-4">
                  <button type="button" className="btn-secondary text-sm" onClick={() => setHardwareDrawerOpen(true)}>
                    Open drawer
                  </button>
                  <Link to={cinemaHubPath(id)} className="btn-secondary text-sm inline-flex items-center gap-1">
                    <Monitor className="w-4 h-4" /> Cinema
                  </Link>
                </div>
              </MacGlassPanel>
            </div>
          )}

          {tab === 'hardware' && vm.inventory_source !== 'kubevirt' && id && (
            <div className="space-y-4 pt-2" data-testid="vm-hardware-tab">
              <MacGlassPanel title="Libvirt hardware" subtitle="Domain XML summary — edit without leaving VM detail">
                {hardware.loading && !hardware.summary ? (
                  <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
                ) : hardware.summary || hardware.report ? (
                  <div className="space-y-4">
                    <div className="rounded-lg border border-white/[0.08] bg-[var(--apple-surface)] px-3 py-1">
                      <VmHardwareSection label="CPU" value={hardware.report?.cpu.value ?? hardware.summary!.cpu} badges={hardware.report?.cpu.badges} testId="vm-hardware-cpu" />
                      <VmHardwareSection label="Memory" value={hardware.report?.memory.value ?? hardware.summary!.memory} badges={hardware.report?.memory.badges} badge={hardware.pending?.needs_shutdown && !hardware.report ? 'restart' : null} />
                      <VmHardwareSection label="Firmware" value={hardware.report?.firmware.value ?? hardware.summary!.firmware} badges={hardware.report?.firmware.badges} />
                      <VmHardwareSection label="TPM" value={hardware.report?.tpm.value ?? hardware.summary!.tpm} badges={hardware.report?.tpm.badges} />
                      <VmHardwareSection label="Display" value={hardware.report?.display.value ?? hardware.summary!.display} badges={hardware.report?.display.badges} />
                      <VmHardwareSection label="NIC" value={hardware.report?.nic.value ?? hardware.summary!.nic} badges={hardware.report?.nic.badges} />
                      <VmHardwareSection label="Guest agent" value={hardware.report?.guest_agent.value ?? hardware.summary!.guestAgent} badges={hardware.report?.guest_agent.badges} />
                      <VmHardwareSection label="Migration" value={hardware.report?.migration.value ?? hardware.summary!.migration} badges={hardware.report?.migration.badges} />
                    </div>
                    {hardware.report?.windows_readiness ? (
                      <VmWindowsReadinessPanel
                        report={hardware.report.windows_readiness}
                        rdpExposed={hardware.summary?.rdpExposed}
                        rdpHostPort={hardware.summary?.rdpHostPort}
                      />
                    ) : null}
                  </div>
                ) : (
                  <p className="text-sm text-[var(--text-muted)]">Hardware details unavailable.</p>
                )}
                <div className="flex flex-wrap gap-2 mt-4">
                  <button type="button" className="btn-primary text-sm" disabled={vm.managed === false} onClick={() => setHardwareEditOpen(true)} data-testid="vm-hardware-edit">
                    Edit Hardware
                  </button>
                  <button type="button" className="btn-secondary text-sm" onClick={() => setHardwareDrawerOpen(true)}>
                    Open drawer
                  </button>
                  <Link to={cinemaHubPath(id)} className="btn-secondary text-sm inline-flex items-center gap-1">
                    <Monitor className="w-4 h-4" /> Cinema
                  </Link>
                </div>
              </MacGlassPanel>
            </div>
          )}

          {tab === 'overview' && (
            <div className="space-y-4 animate-fade-in">
              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4 text-sm">
                <InfoCard label="Desired state" value={vm.desired_state} />
                <InfoCard label="Lifecycle" value={vm.lifecycle_phase || 'idle'} />
                <InfoCard label="Project" value={project || 'default'} />
                <InfoCard label="Backup" value={backups.some((b) => b.status === 'completed') ? 'Protected' : 'Not configured'} />
              </div>
              {metrics && (
                <MacGlassPanel title="Live usage" subtitle="Cockpit-style CPU and memory bars">
                  <VmUsageBars
                    cpuPercent={metrics.cpu_percent}
                    memoryUsedMib={metrics.memory_used_mib}
                    memoryTotalMib={vm.memory_mib ?? undefined}
                    vcpuCount={vm.vcpus ?? undefined}
                  />
                  <button type="button" className={`text-xs mt-3 ${hubLinkClasses()}`} onClick={() => setTab('performance')}>Performance details →</button>
                </MacGlassPanel>
              )}
              {vm.inventory_source !== 'kubevirt' && (
                <MacGlassPanel
                  title="Compute"
                  subtitle="CPU topology and memory sizing (current vs maximum)"
                  action={
                    vm.managed !== false ? (
                      <div className="flex gap-2">
                        <button type="button" className="btn-secondary text-xs" onClick={() => setCpuModalOpen(true)}>Edit CPU</button>
                        <button type="button" className="btn-secondary text-xs" onClick={() => setMemoryModalOpen(true)}>Edit memory</button>
                      </div>
                    ) : null
                  }
                >
                  {computeTopologyLoading && !computeTopology ? (
                    <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading compute…</p>
                  ) : computeTopology ? (
                    <dl className="grid gap-3 sm:grid-cols-2 text-sm">
                      <div>
                        <dt className="text-xs text-[var(--text-muted)]">vCPUs (active)</dt>
                        <dd className="text-[var(--text-primary)]">{computeTopology.vcpus}</dd>
                      </div>
                      <div>
                        <dt className="text-xs text-[var(--text-muted)]">Topology</dt>
                        <dd className="text-[var(--text-primary)]">{computeTopology.sockets}×{computeTopology.cores}×{computeTopology.threads}</dd>
                      </div>
                      <div>
                        <dt className="text-xs text-[var(--text-muted)]">Current memory</dt>
                        <dd className="text-[var(--text-primary)]">{Math.round(computeTopology.current_memory_kib / 1024 / 1024)} GiB</dd>
                      </div>
                      <div>
                        <dt className="text-xs text-[var(--text-muted)]">Maximum memory</dt>
                        <dd className="text-[var(--text-primary)]">{Math.round(computeTopology.max_memory_kib / 1024 / 1024)} GiB</dd>
                      </div>
                      {computeTopology.has_vfio_hostdev && (
                        <div className="sm:col-span-2 text-xs text-amber-600/90">
                          VFIO passthrough device attached — live snapshots are blocked while running.
                        </div>
                      )}
                    </dl>
                  ) : (
                    <p className="text-sm text-[var(--text-muted)]">Compute details unavailable.</p>
                  )}
                </MacGlassPanel>
              )}
              {doctor && (
                <div className="flex flex-wrap items-center gap-2 text-sm rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] px-4 py-3">
                  <span className="text-[var(--text-secondary)]">
                    Doctor: <span className="font-semibold text-[var(--link)]">{doctor.score_numeric}/100</span>
                    {(doctor.issues?.length ?? 0) > 0 ? ` · ${doctor.issues.length} issue(s)` : ' · all checks passed'}
                  </span>
                  <button type="button" className={`text-xs ${hubLinkClasses()}`} onClick={() => setTab('doctor')}>
                    Full report →
                  </button>
                  <button type="button" className="btn-secondary text-xs" disabled={doctorLoading} onClick={() => void runDoctor()}>
                    {doctorLoading ? 'Scanning…' : 'Rescan'}
                  </button>
                </div>
              )}
              {vm.host_id && (
                <Link to={`/platform/hosts/${vm.host_id}`} className={`text-sm inline-flex items-center gap-1 min-h-9 ${hubLinkClasses()}`}>
                  View host resources →
                </Link>
              )}
              {vm.observed_state !== 'running' && (
                <VmOverviewTroubleshootPanel vmId={id!} vmName={vm.name} onOpenDoctor={() => setTab('doctor')} />
              )}
            </div>
          )}

          <VmSshConnectDialog
            open={sshDialogOpen}
            vmName={vm.name}
            platformVmId={id}
            defaultIp={guestIp}
            defaultUser={sshUser}
            detectedIps={guestIp ? [guestIp] : []}
            hypervisorAddress={hypervisorAddress}
            guestIpPrivate={guestAccess?.guest_ip_private}
            portForwardRules={portForwardRules}
            onRefreshPortForwards={() => void loadPortForwards()}
            onClose={() => setSshDialogOpen(false)}
            onConnect={(h, u, p) => navigateVmSshSession(vm.name, h, u, id, p)}
            onNotify={(m) => toast.success(m)}
          />

          {tab === 'doctor' && (
            <div className="pt-2 space-y-3">
              <MachinaExplainObjectPanel kind="vm" id={id!} name={vm.name} showOpenLink={false} />
              <div className="flex justify-end">
                <ExplainButton screen="vm_doctor" objectRef={{ vm_id: id, score: doctor?.score_numeric }} />
              </div>
              {vm && <MachinaVmOutageRca vmId={id!} vmName={vm.name} />}
              <MachinaDoctorPanel
                vmId={id}
                report={doctor}
                loading={doctorLoading}
                onRefresh={() => void runDoctor()}
                onTab={(t) => setTab(t as VmDetailTab)}
              />
              {info?.guestkit?.enabled && (
                <MacGlassPanel title="GuestKit offline migration" subtitle="Disk-based KVM migrate plan without a running guest agent">
                  <p className="text-sm text-[var(--text-muted)] mb-3">
                    Score boot blockers and required changes for live migration using the VM disk image while stopped or online.
                  </p>
                  <div className="flex flex-wrap gap-2">
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      onClick={() => setTab('guestHealth', { guestAction: 'migrate-plan' })}
                    >
                      Run migrate plan
                    </button>
                    <button type="button" className={`btn-secondary text-sm ${hubLinkClasses()}`} onClick={() => setTab('guestHealth')}>
                      Offline assurance
                    </button>
                    <Link to="/platform/migration" className={`btn-secondary text-sm ${hubLinkClasses()}`}>
                      Migration hub
                    </Link>
                  </div>
                </MacGlassPanel>
              )}
              <MachinaVmTroubleshootPanel vmId={id!} vmName={vm?.name} />
            </div>
          )}

          {tab === 'console' && (
            <>
            <div className="pt-2">
              <VmConsoleHeroPreview
                vmName={vm.name}
                vmState={vm.observed_state}
                consoleHref={cinemaHubPath(id!)}
                platformVmId={id}
              />
            </div>
            <div className="space-y-4 pt-2">
              <MacGlassPanel title="VNC console">
                <p className="text-sm text-[var(--text-muted)] mb-4">
                  Machina Cinema — immersive full-screen VNC/SPICE with floating controls.
                </p>
                <div className="flex flex-wrap gap-2">
                  <Link to={cinemaHubPath(id!)} className="btn-primary text-sm inline-flex items-center gap-2">
                    <Monitor className="w-4 h-4" /> Open Cinema
                  </Link>
                  <Link to={studioHubPath(id!)} className="btn-secondary text-sm inline-flex items-center gap-2">
                    Studio
                  </Link>
                </div>
              </MacGlassPanel>
              <MacGlassPanel title="AI terminal tips" subtitle="Zyra-suggested commands for this VM">
                <AiTerminalSuggestStrip vmId={id} vmName={vm.name} compact />
              </MacGlassPanel>
            </div>
            </>
          )}

          {tab === 'performance' && (
            <VmPerfPanel
              series={metricSeries}
              memoryTotalMib={vm.memory_mib ?? undefined}
              vcpuCount={vm.vcpus ?? undefined}
            />
          )}

          {tab === 'devices' && (vm.inventory_source === 'kubevirt' ? (
            <PlatformEmptyState title="Not available" subtitle="Device management requires a libvirt-managed VM." />
          ) : (
            <VmDevicesPanel
              vmId={id}
              hostId={vm.host_id}
              details={libvirtDetails}
              domainXml={domainXml}
              loading={libvirtDetailsLoading}
              vmState={vm.observed_state}
              onChanged={() => void loadDomainXml()}
            />
          ))}

          {tab === 'disks' && (vm.inventory_source === 'kubevirt' ? (
            <PlatformEmptyState title="Not available" subtitle="Disk management requires a libvirt-managed VM." />
          ) : (
            <div className="space-y-4 pt-2" data-testid="vm-disks-panel">
              <MacGlassPanel title="Libvirt disks" subtitle="Live hypervisor inventory">
                {libvirtDetailsLoading ? (
                  <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
                ) : libvirtDetails?.disks.length ? (
                  <ul className="text-sm text-[var(--text-muted)] space-y-3">
                    {libvirtDetails.disks.map((d) => (
                      <li key={d.target} className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.04] pb-2">
                        <span className="flex items-center gap-2 flex-wrap">
                          <span className="font-mono text-[var(--text-primary)]">{d.target}</span>
                          <VmPendingBadge pending={pendingConfig} category="disk" />
                          {' · '}{d.device}
                          {d.bus ? ` · ${d.bus}` : ''}
                          {d.cache ? ` · cache ${d.cache}` : ''}
                          {d.source ? ` · ${d.source}` : ''}
                          {d.capacity_bytes ? ` · ${formatBytes(d.capacity_bytes)} cap` : ''}
                          {d.physical_bytes ? ` · ${formatBytes(d.physical_bytes)} on host` : ''}
                        </span>
                        <div className="flex gap-2">
                          {d.device === 'disk' && (
                            <>
                              <button
                                type="button"
                                className="btn-secondary text-xs"
                                disabled={vm.managed === false}
                                onClick={() => {
                                  setDiskEditTarget(d.target)
                                  setDiskEditCache(d.cache || 'none')
                                  setDiskEditBus(d.bus || 'virtio')
                                  setDiskEditReadonly(Boolean(d.readonly))
                                }}
                              >
                                Edit
                              </button>
                              <button
                                type="button"
                                className="btn-secondary text-xs"
                                disabled={vm.managed === false}
                                onClick={() => void act(`Detach ${d.target} queued`, () => detachVmDisk(id, d.target))}
                              >
                                Detach
                              </button>
                            </>
                          )}
                          {d.device === 'cdrom' && d.source && (
                            <button
                              type="button"
                              className="btn-secondary text-xs"
                              disabled={vm.managed === false}
                              data-testid={`cdrom-eject-${d.target}`}
                              onClick={() => void act('CD-ROM ejected', () => invokeVmLibvirt(id, 'cdrom.eject', { target: d.target }))}
                            >
                              Eject
                            </button>
                          )}
                        </div>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="text-sm text-[var(--text-muted)]">No disks reported from libvirt.</p>
                )}
              </MacGlassPanel>
              {diskEditTarget && (
                <MacGlassPanel title={`Edit disk ${diskEditTarget}`}>
                  <div className="flex flex-wrap gap-3 items-end">
                    <label className="text-xs text-[var(--text-muted)]">
                      Bus
                      <select className="input mt-1 block" value={diskEditBus} onChange={(e) => setDiskEditBus(e.target.value)}>
                        <option value="virtio">virtio</option>
                        <option value="sata">sata</option>
                        <option value="scsi">scsi</option>
                        <option value="ide">ide</option>
                      </select>
                    </label>
                    <label className="text-xs text-[var(--text-muted)]">
                      Cache
                      <select className="input mt-1 block" value={diskEditCache} onChange={(e) => setDiskEditCache(e.target.value)}>
                        <option value="none">none</option>
                        <option value="writethrough">writethrough</option>
                        <option value="writeback">writeback</option>
                        <option value="directsync">directsync</option>
                        <option value="unsafe">unsafe</option>
                      </select>
                    </label>
                    <label className="text-xs text-[var(--text-muted)] flex items-center gap-2 mt-5">
                      <input type="checkbox" checked={diskEditReadonly} onChange={(e) => setDiskEditReadonly(e.target.checked)} />
                      Read-only
                    </label>
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      onClick={() => void act('Disk updated', async () => {
                        await invokeVmLibvirt(id, 'disk.tune', {
                          target: diskEditTarget,
                          bus: diskEditBus,
                          cache: diskEditCache,
                          readonly: diskEditReadonly,
                        })
                        setDiskEditTarget(null)
                        await loadLibvirtDetails()
                        await loadPendingConfig()
                      })}
                    >
                      Apply
                    </button>
                    <button type="button" className="btn-secondary text-sm" onClick={() => setDiskEditTarget(null)}>Cancel</button>
                  </div>
                </MacGlassPanel>
              )}
              {disks.length > 0 && (
                <MacGlassPanel title="Platform disk records">
                  <ul className="text-sm text-[var(--text-muted)] space-y-2">{disks.map((d) => (
                    <li key={d.id}>{d.name} · {d.size_gib} GiB · {d.storage_class}{d.path ? ` · ${d.path}` : ''}</li>
                  ))}</ul>
                </MacGlassPanel>
              )}
              <MacGlassPanel title="Attach disk">
                <div className="flex flex-wrap gap-3 items-end">
                  <label className="text-xs text-[var(--text-muted)]">
                    Path
                    <div className="mt-1 flex gap-2 min-w-[18rem]">
                      <input className="input flex-1 min-w-0 font-mono text-xs" value={attachPath} onChange={(e) => setAttachPath(e.target.value)} />
                      <button
                        type="button"
                        className="btn-secondary shrink-0 inline-flex items-center gap-1.5 text-xs"
                        data-testid="vm-attach-disk-browse"
                        disabled={!canBrowseHost}
                        title={!canBrowseHost ? 'Browsing host paths requires the admin role' : undefined}
                        onClick={() => {
                          if (canBrowseHost) setAttachDiskBrowseOpen(true)
                        }}
                      >
                        <FolderOpen className="w-3.5 h-3.5" aria-hidden />
                        Browse
                      </button>
                    </div>
                  </label>
                  <label className="text-xs text-[var(--text-muted)]">Target dev<input className="input mt-1 block w-24" value={attachDev} onChange={(e) => setAttachDev(e.target.value)} /></label>
                  <button type="button" className="btn-secondary text-sm" disabled={vm.managed === false} onClick={() => void act('Attach disk queued', () => attachVmDisk(id, { disk_path: attachPath, target_dev: attachDev }))}>Attach</button>
                </div>
              </MacGlassPanel>
              <MacGlassPanel title="Resize block device">
                <div className="flex flex-wrap gap-3 items-end">
                  <label className="text-xs text-[var(--text-muted)]">
                    Target
                    <select className="input mt-1 block min-w-[8rem]" value={resizeTarget} onChange={(e) => setResizeTarget(e.target.value)}>
                      <option value="">Select…</option>
                      {(libvirtDetails?.disks ?? []).filter((d) => d.device === 'disk').map((d) => (
                        <option key={d.target} value={d.target}>{d.target}</option>
                      ))}
                    </select>
                  </label>
                  <label className="text-xs text-[var(--text-muted)]">Size (GiB)<input type="number" min={1} className="input mt-1 block w-24" value={resizeGb} onChange={(e) => setResizeGb(e.target.value)} /></label>
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    disabled={vm.managed === false || !resizeTarget || !resizeGb || Number.isNaN(Number(resizeGb))}
                    onClick={() => void act('Resize disk queued', () => resizeVmDisk(id, resizeTarget, Number(resizeGb)))}
                  >
                    Resize
                  </button>
                </div>
              </MacGlassPanel>
              <MacGlassPanel title="Insert ISO" subtitle="Attach host ISO to a CD-ROM target (live when running)" data-testid="vm-insert-iso-panel">
                <div className="flex flex-wrap gap-3 items-end">
                  <label className="text-xs text-[var(--text-muted)]">
                    ISO path
                    <div className="mt-1 space-y-2 min-w-[18rem]">
                      {isoFiles.length > 0 ? (
                        <select
                          className="input block w-full text-xs"
                          data-testid="vm-insert-iso-scan"
                          value={isoPath}
                          onChange={(e) => setIsoPath(e.target.value)}
                        >
                          <option value="">Select ISO (from scan)…</option>
                          {isoFiles.map((f) => (
                            <option key={f.path} value={f.path}>
                              {f.name} ({(f.size_bytes / 1048576).toFixed(0)} MB)
                            </option>
                          ))}
                        </select>
                      ) : null}
                      <div className="flex gap-2">
                        <input
                          aria-label="ISO path"
                          className="input flex-1 min-w-0 font-mono text-xs"
                          value={isoPath}
                          onChange={(e) => setIsoPath(e.target.value)}
                          placeholder="/var/lib/libvirt/images/debian-12.iso"
                        />
                        <button
                          type="button"
                          className="btn-secondary shrink-0 inline-flex items-center gap-1.5 text-xs"
                          data-testid="vm-insert-iso-browse"
                          disabled={!canBrowseHost}
                          title={!canBrowseHost ? 'Browsing host paths requires the admin role' : undefined}
                          onClick={() => {
                            if (canBrowseHost) setIsoBrowseOpen(true)
                          }}
                        >
                          <FolderOpen className="w-3.5 h-3.5" aria-hidden />
                          Browse
                        </button>
                      </div>
                    </div>
                  </label>
                  <label className="text-xs text-[var(--text-muted)]">
                    CD-ROM target
                    <select className="input mt-1 block w-24" value={isoTarget} onChange={(e) => setIsoTarget(e.target.value)}>
                      {cdromTargets.map((t) => (
                        <option key={t} value={t}>{t}</option>
                      ))}
                    </select>
                  </label>
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    data-testid="vm-insert-iso-submit"
                    disabled={vm.managed === false || !isoPath.trim()}
                    onClick={() => void act('ISO inserted', () => invokeVmLibvirt(id, 'cdrom.insert', { iso_path: isoPath.trim(), target: isoTarget }))}
                  >
                    Insert ISO
                  </button>
                </div>
                <p className="text-xs text-[var(--text-muted)] mt-2">ISO scan, host browse, or typed path — same libvirt <code className="text-[var(--text-muted)]">cdrom.insert</code> as classic VM detail.</p>
              </MacGlassPanel>
            </div>
          ))}

          {tab === 'network' && vm.inventory_source !== 'kubevirt' && (
            <div className="space-y-4 pt-2" data-testid="vm-network-panel">
              <MacGlassPanel title="Network interfaces" subtitle="Hot-plug NICs via libvirt">
                {libvirtDetailsLoading ? (
                  <p className="text-sm text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-4 h-4 animate-spin" /> Loading…</p>
                ) : libvirtDetails?.interfaces.length ? (
                  <ul className="text-sm text-[var(--text-muted)] space-y-3">
                    {libvirtDetails.interfaces.map((iface) => (
                      <li key={iface.mac_address} className="flex flex-wrap items-center justify-between gap-2 border-b border-white/[0.04] pb-2">
                        <span className="flex items-center gap-2 flex-wrap">
                          <Network className="w-4 h-4 text-[var(--text-muted)]" />
                          {iface.mac_address} · {iface.source} · {iface.model}
                          {iface.ip && (
                            <span className="font-mono text-emerald-600/80"> · {iface.ip}</span>
                          )}
                          <VmPendingBadge pending={pendingConfig} category="network" />
                        </span>
                        <button
                          type="button"
                          className="btn-secondary text-xs"
                          disabled={vm.managed === false}
                          onClick={() => {
                            setNicEditMac(iface.mac_address)
                            setNicEditModel(iface.model || 'virtio')
                            setNicEditNetwork(iface.source || 'default')
                          }}
                        >
                          Edit
                        </button>
                        <button
                          type="button"
                          className="btn-secondary text-xs"
                          disabled={vm.managed === false}
                          onClick={() => void act('Detach NIC queued', () => detachVmNic(id, iface.mac_address))}
                        >
                          Detach
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="text-sm text-[var(--text-muted)]">No interfaces attached.</p>
                )}
                {nicEditMac && (
                  <div className="mt-4 p-3 rounded-lg border border-white/[0.06] space-y-3">
                    <p className="text-sm text-[var(--text-secondary)]">Edit NIC <span className="font-mono">{nicEditMac}</span></p>
                    <div className="flex flex-wrap gap-3 items-end">
                      <label className="text-xs text-[var(--text-muted)]">
                        Network
                        <input className="input mt-1 block min-w-[12rem]" value={nicEditNetwork} onChange={(e) => setNicEditNetwork(e.target.value)} />
                      </label>
                      <label className="text-xs text-[var(--text-muted)]">
                        Model
                        <select className="input mt-1 block w-28" value={nicEditModel} onChange={(e) => setNicEditModel(e.target.value)}>
                          <option value="virtio">virtio</option>
                          <option value="e1000">e1000</option>
                          <option value="e1000e">e1000e</option>
                          <option value="rtl8139">rtl8139</option>
                        </select>
                      </label>
                      <button
                        type="button"
                        className="btn-secondary text-sm"
                        onClick={() => void act('NIC updated', async () => {
                          await invokeVmLibvirt(id, 'nic.tune', {
                            mac_address: nicEditMac,
                            model: nicEditModel,
                            network: nicEditNetwork,
                          })
                          setNicEditMac(null)
                          await loadLibvirtDetails()
                          await loadPendingConfig()
                        })}
                      >
                        Apply
                      </button>
                      <button type="button" className="btn-secondary text-sm" onClick={() => setNicEditMac(null)}>Cancel</button>
                    </div>
                  </div>
                )}
                <div className="flex flex-wrap gap-3 items-end mt-4 pt-3 border-t border-white/[0.04]">
                  <label className="text-xs text-[var(--text-muted)]">
                    Network
                    {platformNetworks.length > 0 ? (
                      <select className="input mt-1 block min-w-[12rem]" value={nicNetwork} onChange={(e) => setNicNetwork(e.target.value)}>
                        {platformNetworks.map((n) => <option key={n.name} value={n.name}>{n.name}</option>)}
                      </select>
                    ) : (
                      <input className="input mt-1 block min-w-[12rem]" value={nicNetwork} onChange={(e) => setNicNetwork(e.target.value)} />
                    )}
                  </label>
                  <label className="text-xs text-[var(--text-muted)]">
                    Model
                    <select className="input mt-1 block w-28" value={nicModel} onChange={(e) => setNicModel(e.target.value)}>
                      <option value="virtio">virtio</option>
                      <option value="e1000">e1000</option>
                      <option value="e1000e">e1000e</option>
                      <option value="rtl8139">rtl8139</option>
                    </select>
                  </label>
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    disabled={vm.managed === false || !nicNetwork.trim()}
                    onClick={() => void act('Attach NIC queued', () => attachVmNic(id, { network: nicNetwork.trim(), model: nicModel }))}
                  >
                    Attach NIC
                  </button>
                </div>
              </MacGlassPanel>
              <MacGlassPanel title="Hypervisor NAT (port forwards)">
                <VmPortForwardPanel
                  platformVmId={id!}
                  vmName={vm.name}
                  guestIp={guestIp}
                  sshUser={sshUser}
                  hypervisorAddress={hypervisorAddress}
                  onNotify={(msg) => toast.success(msg)}
                />
              </MacGlassPanel>
              <MacGlassPanel title="Platform networks">
                <p className="text-sm text-[var(--text-muted)]">Attach NICs to libvirt networks provisioned on the host.</p>
                <Link to="/platform/networks" className={`text-sm mt-2 inline-block ${hubLinkClasses()}`}>Manage networks →</Link>
              </MacGlassPanel>
            </div>
          )}
          {tab === 'network' && vm.inventory_source === 'kubevirt' && (
            <MacGlassPanel title="Network" className="pt-2">
              <p className="text-sm text-[var(--text-muted)]">KubeVirt networking is managed via the cluster CNI. Use kubectl or the K8s console for service exposure.</p>
            </MacGlassPanel>
          )}

          {tab === 'guestHealth' && (
            <div className="space-y-4 pt-2">
              <MacGlassPanel title="Guest OS health" subtitle="Live QGA · GuestKit offline disk · cloud-init">
                <GuestAgentDiagnosticsPanel
                  vmId={id!}
                  vmName={vm.name}
                  loading={guestHealthLoading}
                  report={guestHealth}
                  error={guestHealthError}
                  vmState={vm.observed_state}
                  lastRefreshedAt={guestHealthRefreshedAt}
                  onRefresh={() => void loadGuestHealth({ quiet: Boolean(guestHealth) })}
                  onStartVm={
                    vm.observed_state === 'stopped' || vm.observed_state === 'shut off' || vm.observed_state === 'shutoff'
                      ? () => void act('Start queued', () => vmPower(id, 'start'))
                      : undefined
                  }
                  onInstall={
                    vm.observed_state === 'running' ? () => void queueGuestToolsInstall() : undefined
                  }
                  installing={guestInstalling}
                  onRunAction={runGuestAction}
                  portForwardRules={portForwardRules}
                  hypervisorAddress={hypervisorAddress}
                  onPortForwardRefresh={() => void loadPortForwards()}
                />
                <GuestObservabilityStrip
                  vmId={id!}
                  className="mt-4"
                  initial={guestHealth?.guest_observability}
                />
                <GuestFsFreezeBanner vmId={id!} className="mt-3" />
                <div className="mt-4">
                  <GuestkitOfflineAssurancePanel
                    vmId={id!}
                    vmState={vm.observed_state}
                    guestkitEnabled={Boolean(info?.guestkit?.enabled)}
                    autoRunMigratePlan={guestMigratePlanAction}
                  />
                </div>
                <OsDiagnosePanel
                  resourceId={id!}
                  resourceKind="vm"
                  defaultQuery="guest health and agent connectivity"
                  loading={vmDiagnoseLoading}
                  report={vmDiagnose}
                  onRun={(q) => void runVmDiagnose(q)}
                  onAskCopilot={openCopilot}
                />
              </MacGlassPanel>
              <GuestAiInsightsPanel
                vmId={id!}
                autoLoad
                onApplied={() => void loadGuestHealth()}
                onRunAction={runGuestAction}
              />
            </div>
          )}

          {tab === 'guestServices' && (
            <div className="space-y-4 pt-2">
              <MacGlassPanel title="Guest services" subtitle="Live guestkit-agent inventory · start/stop controllable units">
                {guestServicesLoading && (
                  <p className="text-sm text-[var(--text-muted)] flex items-center gap-2">
                    <Loader2 className="w-4 h-4 animate-spin" /> Loading guest services…
                  </p>
                )}
                {!guestServicesLoading && guestServicesError && (
                  <div className={`rounded-lg border p-3 text-sm ${statusSurfaceClasses('error')}`}>
                    <p className="text-[var(--text-primary)]">{guestServicesError}</p>
                    <button type="button" className="btn-secondary text-xs mt-2" onClick={() => void loadGuestServices()}>
                      Retry
                    </button>
                  </div>
                )}
                {!guestServicesLoading && !guestServicesError && guestServices && (
                  <>
                    <p className="text-xs text-[var(--text-muted)] mb-3">
                      {guestServices.summary}
                      {!guestServices.agent_reachable ? ' · Guest agent unreachable' : ''}
                    </p>
                    {(guestServices.services ?? []).length === 0 ? (
                      <PlatformEmptyState
                        icon={Server}
                        title="No guest services"
                        subtitle="The guest agent did not report any service inventory for this VM."
                      />
                    ) : (
                      (guestServices.services ?? []).map((s, i) => (
                        <MacListRow
                          key={`${s.name}-${i}`}
                          title={s.name}
                          subtitle={`${s.status} · ${s.detail}`}
                          trailing={
                            s.controllable ? (
                              <div className="flex gap-1">
                                <button
                                  type="button"
                                  className="btn-secondary text-xs px-2 py-0.5"
                                  disabled={!!guestServiceBusy}
                                  onClick={() => void runGuestServiceAction(s.name, 'start')}
                                >
                                  {guestServiceBusy === `${s.name}:start` ? (
                                    <Loader2 className="w-3 h-3 animate-spin inline" />
                                  ) : null}
                                  Start
                                </button>
                                <button
                                  type="button"
                                  className="btn-secondary text-xs px-2 py-0.5"
                                  disabled={!!guestServiceBusy}
                                  onClick={() => void runGuestServiceAction(s.name, 'stop')}
                                >
                                  {guestServiceBusy === `${s.name}:stop` ? (
                                    <Loader2 className="w-3 h-3 animate-spin inline" />
                                  ) : null}
                                  Stop
                                </button>
                                <button
                                  type="button"
                                  className="btn-secondary text-xs px-2 py-0.5"
                                  disabled={!!guestServiceBusy}
                                  onClick={() => void runGuestServiceAction(s.name, 'restart')}
                                >
                                  {guestServiceBusy === `${s.name}:restart` ? (
                                    <Loader2 className="w-3 h-3 animate-spin inline" />
                                  ) : null}
                                  Restart
                                </button>
                              </div>
                            ) : undefined
                          }
                        />
                      ))
                    )}
                  </>
                )}
                {!guestServicesLoading && !guestServicesError && !guestServices && (
                  <PlatformEmptyState
                    icon={Server}
                    title="Guest services unavailable"
                    subtitle="Start the VM and ensure the guest agent is active, then refresh."
                    action={
                      <button type="button" className="btn-secondary text-xs" onClick={() => void loadGuestServices()}>
                        Refresh
                      </button>
                    }
                  />
                )}
                <button type="button" className="btn-secondary text-xs mt-3" onClick={() => void loadGuestServices()}>Refresh</button>
                <OsDiagnosePanel
                  resourceId={id!}
                  resourceKind="vm"
                  defaultQuery="guest services and exposed ports"
                  loading={vmDiagnoseLoading}
                  report={vmDiagnose}
                  onRun={(q) => void runVmDiagnose(q)}
                  onAskCopilot={openCopilot}
                />
              </MacGlassPanel>
            </div>
          )}

          {tab === 'security' && (
            <div className="space-y-4 pt-2">
              <div className="flex justify-end">
                <ExplainButton screen="guest_ports" objectRef={{ vm_id: id }} />
              </div>
              <MacGlassPanel
                title="Security"
                subtitle="In-guest listening ports via QEMU guest agent · host firewall on parent machine"
              >
                {vm.host_id && (
                  <Link to={`/platform/zeus/security/firewall/${vm.host_id}`} className={`text-sm inline-flex items-center gap-1 mb-4 ${hubLinkClasses()}`}>
                    <Shield className="w-4 h-4" /> Host firewall (Zeus) →
                  </Link>
                )}
                {guestPortsLoading && <p className="text-sm text-[var(--text-muted)]">Loading guest ports…</p>}
                {!guestPortsLoading && guestPorts && (
                  <>
                    <p className="text-xs text-[var(--text-muted)] mb-3">
                      {guestPorts.summary}
                      {!guestPorts.agent_reachable && ' · Guest agent unreachable — install Guest Tools'}
                    </p>
                    {(guestPorts.ports ?? []).length === 0 ? (
                      <p className="text-sm text-[var(--text-muted)]">No listening ports reported inside the guest.</p>
                    ) : (
                      <div className="space-y-1">
                        {(guestPorts.ports ?? []).map((p) => (
                          <MacListRow
                            key={`${p.port}-${p.protocol}`}
                            title={`${p.port}/${p.protocol}`}
                            subtitle={[p.service_name || p.process, String(p.risk)].filter(Boolean).join(' · ')}
                            badge={
                              (p.risk === 'Critical' || p.risk === 'critical') ? (
                                <span className={`text-[10px] px-2 py-0.5 rounded-full ${statusBadgeClasses('error')}`}>critical</span>
                              ) : (p.risk === 'Warning' || p.risk === 'warning') ? (
                                <span className={`text-[10px] px-2 py-0.5 rounded-full ${statusBadgeClasses('warn')}`}>warning</span>
                              ) : undefined
                            }
                          />
                        ))}
                      </div>
                    )}
                  </>
                )}
                {!guestPortsLoading && !guestPorts && (
                  <p className="text-sm text-[var(--text-muted)]">Could not load guest firewall ports. Ensure Guest Tools are installed and the VM is running.</p>
                )}
                <button type="button" className="btn-secondary text-xs mt-3" onClick={() => void loadGuestPorts()}>Refresh</button>
              </MacGlassPanel>
            </div>
          )}

          {tab === 'snapshots' && (
            <MacGlassPanel title="Snapshots & Time Machine" className="pt-2" data-testid="vm-snapshots-panel">
              {timeline.length > 0 && (
                <div className="mb-4 pb-4 border-b border-white/5">
                  <h3 className="text-sm font-semibold mb-2">Time Machine</h3>
                  <ul className="text-xs space-y-2">
                    {timeline.map((e) => (
                      <li key={`${e.kind}-${e.id}`} className="flex flex-wrap items-center justify-between gap-2 text-[var(--text-muted)]">
                        <span>
                          <span className="text-[var(--text-muted)] uppercase text-[10px] mr-1">{e.kind}</span>
                          {e.label} · {new Date(e.created_at).toLocaleString()}
                        </span>
                        {e.kind === 'snapshot' && (
                          <button
                            type="button"
                            className="btn-secondary text-xs"
                            onClick={() => {
                              const name = e.label.replace(/^Snapshot:\s*/, '')
                              setSnapDestructiveConfirm({ snapName: name, action: 'revert', run: () => revertVmSnapshot(id, name), label: 'Revert queued' })
                            }}
                          >
                            Revert
                          </button>
                        )}
                        {e.kind === 'backup' && e.status === 'completed' && (
                          <button type="button" className="btn-secondary text-xs" onClick={() => void act('Restore queued', () => restoreVmBackup(id, e.id))}>
                            Restore
                          </button>
                        )}
                      </li>
                    ))}
                  </ul>
                </div>
              )}
              <input aria-label="Snapshot name" className="input w-full max-w-xs" value={snapName} onChange={(e) => setSnapName(e.target.value)} placeholder="snap-01" />
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  className="btn-secondary text-xs"
                  disabled={snapAiLoading || vm.observed_state !== 'running'}
                  onClick={() => {
                    setSnapAiLoading(true)
                    void getVmGuestAiInsights(id, { focus: 'snapshot' })
                      .then((r) => {
                        setSnapAiHint(r)
                        const quiesceRec = (r.recommendations ?? []).find((x) => x.action === 'snapshot.quiesce')
                        if (quiesceRec) setSnapQuiesce(true)
                      })
                      .catch((e: unknown) => toast.error(formatUserError(e)))
                      .finally(() => setSnapAiLoading(false))
                  }}
                >
                  AI snapshot advice
                </button>
                {snapAiHint && (
                  <p className="text-xs text-[var(--text-muted)] max-w-xl">{snapAiHint.summary}</p>
                )}
              </div>
              <div className="flex flex-wrap gap-4 mt-3 text-xs text-[var(--text-muted)]">
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={snapDiskOnly} onChange={(e) => setSnapDiskOnly(e.target.checked)} /> Disk only
                </label>
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={snapQuiesce} onChange={(e) => setSnapQuiesce(e.target.checked)} /> Guest quiesce
                </label>
                <select aria-label="Snapshot storage mode" className="input text-xs max-w-[140px]" value={snapStorageMode} onChange={(e) => setSnapStorageMode(e.target.value)}>
                  <option value="">Storage: auto</option>
                  <option value="internal">Internal</option>
                  <option value="external">External</option>
                </select>
              </div>
              {snapQuiesce && vm.observed_state === 'running' && (
                <GuestFsFreezeBanner vmId={id!} poll className="mt-3" />
              )}
              {(snapPrecheckLoading || snapPrecheck) && (
                <div className="mt-3 rounded-lg border border-white/[0.06] bg-[var(--apple-surface)] p-3 text-xs">
                  {snapPrecheckLoading ? (
                    <p className="text-[var(--text-muted)] flex items-center gap-2"><Loader2 className="w-3 h-3 animate-spin" /> Checking snapshot readiness…</p>
                  ) : snapPrecheck?.blocked ? (
                    <p className="text-amber-600/90">{snapPrecheck.message}</p>
                  ) : snapPrecheck?.has_vfio_hostdev && vm.observed_state === 'running' ? (
                    <p className="text-amber-600/90">VFIO device present — stop the VM before creating a snapshot.</p>
                  ) : snapPrecheck?.estimated_bytes ? (
                    <p className="text-[var(--text-muted)]">
                      External snapshot may need ~{Math.ceil(snapPrecheck.estimated_bytes / (1024 * 1024))} MiB
                      {snapPrecheck.available_bytes != null && ` (${Math.floor(snapPrecheck.available_bytes / (1024 * 1024))} MiB free on host)`}.
                    </p>
                  ) : (
                    <p className="text-[var(--text-muted)]">Snapshot precheck passed.</p>
                  )}
                </div>
              )}
              <div className="flex flex-wrap gap-2 mt-2">
                <button
                  type="button"
                  className="btn-secondary text-xs"
                  disabled={!!snapPrecheck?.blocked}
                  onClick={() => {
                    const body: CreateVmSnapshotBody = {
                      name: snapName.trim() || 'snap-01',
                      disk_only: snapDiskOnly,
                      quiesce: snapQuiesce,
                      storage_mode: snapStorageMode || undefined,
                    }
                    void act('Snapshot queued', () => createVmSnapshot(id, body))
                  }}
                >
                  Create snapshot
                </button>
              </div>
              <ul className="text-xs space-y-3 mt-3">
                {snapshots.map((s) => (
                  <li key={s.id} className="flex flex-col gap-2 text-[var(--text-muted)] border-b border-white/5 pb-2">
                    <span>{s.name} ({s.status})</span>
                    <span className="flex flex-wrap gap-1">
                      <button type="button" className="btn-secondary text-xs" onClick={() => setSnapDestructiveConfirm({ snapName: s.name, action: 'revert', run: () => revertVmSnapshot(id, s.name), label: 'Revert queued' })}>Revert</button>
                      <button type="button" className="btn-secondary text-xs" onClick={() => setSnapDestructiveConfirm({ snapName: s.name, action: 'delete', run: () => deleteVmSnapshot(id, s.name), label: 'Delete queued' })}>Delete</button>
                    </span>
                    <button
                      type="button"
                      className="btn-secondary text-xs w-fit"
                      onClick={() => void runSnapshotAction(s.name, 'clone', () => cloneVmSnapshot(id, s.name, `${vm.name}-from-${s.name}`), 'Clone queued')}
                    >
                      Clone to {vm.name}-from-{s.name}
                    </button>
                  </li>
                ))}
              </ul>
            </MacGlassPanel>
          )}

          {tab === 'backup' && (
            <MacGlassPanel title="Backup" className="pt-2">
              <p className="text-xs text-[var(--text-muted)]">Restore will not overwrite the current VM unless you choose restore in place.</p>
              <button type="button" className="btn-secondary text-xs" onClick={() => void act('Backup queued', () => createVmBackup(id))}><Archive className="w-3 h-3 inline" /> Backup now</button>
              <ul className="text-xs space-y-2">
                {backups.map((b) => (
                  <li key={b.id} className="flex items-center justify-between gap-2 text-[var(--text-muted)]">
                    <span className="min-w-0">{b.backup_type} ({b.status}){b.status === 'completed' ? <> · <BackupHealthChip backup={b} /></> : null}</span>
                    {b.status === 'completed' && (
                      <span className="flex shrink-0 gap-2">
                        <button type="button" className="btn-secondary text-xs" onClick={() => void act('Backup re-checked', () => verifyVmBackup(id, b.id))}>Verify now</button>
                        <button type="button" className="btn-secondary text-xs" onClick={() => void act('Restore queued', () => restoreVmBackup(id, b.id))}>Restore</button>
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </MacGlassPanel>
          )}

          {tab === 'topology' && (
            <MacGlassPanel title="VM topology" className="pt-2">
              {!topology ? (
                <p className="text-sm text-[var(--text-muted)]">Loading topology…</p>
              ) : (
                <div className="space-y-3 text-sm">
                  <p className="text-[var(--text-muted)]">{topology.nodes.length} nodes · {topology.edges.length} edges</p>
                  <ul className="divide-y divide-white/[0.04]">
                    {topology.nodes.map((n) => (
                      <li key={n.id} className="py-2 flex justify-between gap-2">
                        <span className="text-[var(--text-primary)]">{n.name}</span>
                        <span className="text-xs text-[var(--text-muted)] uppercase">{n.kind}</span>
                      </li>
                    ))}
                  </ul>
                  <Link to="/platform/topology" className={`text-xs ${hubLinkClasses()}`}>Open fleet topology →</Link>
                </div>
              )}
            </MacGlassPanel>
          )}

          {tab === 'events' && (
            <div className="space-y-4 pt-2">
              <MacGlassPanel title="Migration history">
                {migrations.length === 0 ? (
                  <p className="text-sm text-[var(--text-muted)]">No migration records for this VM.</p>
                ) : (
                  <ul className="divide-y divide-white/[0.04] -mx-1">
                    {migrations.map((m) => (
                      <MacListRow
                        key={m.id}
                        title={`${m.source_host} → ${m.dest_host}`}
                        subtitle={`${m.status} · ${new Date(m.created_at).toLocaleString()}`}
                      />
                    ))}
                  </ul>
                )}
              </MacGlassPanel>
              <MacGlassPanel title="Events">
                <Link to={tasksHubHref(tier)} className={hubLinkClasses()}>View task history →</Link>
              </MacGlassPanel>
            </div>
          )}

          {tab === 'logs' && (vm.inventory_source === 'kubevirt' ? (
            <PlatformEmptyState title="Not available" subtitle="QEMU logs are only available for libvirt-managed VMs." />
          ) : (
            <VmQemuLogsPanel vmId={id} vmName={vm.name} />
          ))}

          {tab === 'settings' && (
            <div className="space-y-4">
              <MacGlassPanel title="Organization">
                <div className="flex flex-wrap gap-3 items-end">
                  <div>
                    <label htmlFor="vm-settings-project" className="text-xs text-[var(--text-muted)] block mb-1">Project</label>
                    <input id="vm-settings-project" className="input" value={project} onChange={(e) => setProject(e.target.value)} placeholder="default" />
                  </div>
                  <div className="flex-1 min-w-[12rem]">
                    <label htmlFor="vm-settings-tags" className="text-xs text-[var(--text-muted)] block mb-1">Tags</label>
                    <input id="vm-settings-tags" className="input w-full" value={tags} onChange={(e) => setTags(e.target.value)} placeholder="prod, web" />
                  </div>
                  <button type="button" className="btn-secondary text-sm" onClick={() => void act('Project updated', () => patchVm(id, { project, tags: tags.split(',').map((t) => t.trim()).filter(Boolean) }))}>Save</button>
                </div>
              </MacGlassPanel>
              <MacGlassPanel title="Description">
                <p className="text-xs text-[var(--text-muted)] mb-2">Operator notes stored in the VM spec (Cockpit Machines parity).</p>
                <textarea
                  aria-label="Description"
                  className="input w-full min-h-[4.5rem] text-sm"
                  value={descriptionDraft}
                  onChange={(e) => setDescriptionDraft(e.target.value)}
                  placeholder="Optional description for this VM"
                  data-testid="vm-description-input"
                />
                <button
                  type="button"
                  className="btn-secondary text-sm mt-2"
                  onClick={() => void act('Description saved', () => patchVm(id, { description: descriptionDraft }))}
                >
                  Save description
                </button>
              </MacGlassPanel>
              {vm.inventory_source !== 'kubevirt' && (vm.observed_state === 'shutoff' || vm.observed_state === 'stopped') && (
                <MacGlassPanel title="Rename VM">
                  <p className="text-xs text-[var(--text-muted)] mb-2">Libvirt domain rename (guest must be shut off).</p>
                  <div className="flex flex-wrap gap-2">
                    <input
                      aria-label="New VM name"
                      className="input flex-1 min-w-[12rem]"
                      value={renameDraft || vm.name}
                      onChange={(e) => setRenameDraft(e.target.value)}
                      placeholder={vm.name}
                    />
                    <button
                      type="button"
                      className="btn-secondary text-sm"
                      disabled={!renameDraft.trim() || renameDraft.trim() === vm.name}
                      onClick={() => void act('VM renamed', async () => {
                        await renamePlatformVm(id, renameDraft.trim())
                        navigate(`/platform/vms/${id}`, { replace: true })
                      })}
                    >
                      Rename
                    </button>
                  </div>
                </MacGlassPanel>
              )}
              {vm.inventory_source !== 'kubevirt' && (
                <VmGraphicsPanel
                  vmId={id}
                  domainXml={domainXml}
                  disabled={vm.managed === false}
                  onChanged={() => void loadDomainXml()}
                />
              )}
              {vm.inventory_source !== 'kubevirt' && (
                <MacGlassPanel title="Boot & autostart">
                  <div className="flex flex-wrap items-center gap-4 mb-4">
                    <span className="text-sm text-[var(--text-muted)]">Autostart on host boot</span>
                    <button
                      type="button"
                      className="inline-flex items-center gap-2 text-sm"
                      disabled={vm.managed === false || libvirtDetailsLoading}
                      onClick={() => void act(
                        libvirtDetails?.autostart ? 'Autostart disabled' : 'Autostart enabled',
                        () => setVmAutostart(id, !libvirtDetails?.autostart),
                      )}
                    >
                      {libvirtDetails?.autostart ? (
                        <><ToggleRight className={`w-5 h-5 ${statusToneClass('ok')}`} /> Enabled</>
                      ) : (
                        <><ToggleLeft className="w-5 h-5 text-[var(--text-muted)]" /> Disabled</>
                      )}
                    </button>
                  </div>
                  <p className="text-xs text-[var(--text-muted)]">
                    CPU topology and memory sizing live on the{' '}
                    <button type="button" className={hubLinkClasses()} onClick={() => setTab('overview')}>Overview</button>
                    {' '}Compute panel — use Edit CPU / Edit memory there.
                  </p>
                </MacGlassPanel>
              )}
              <MacGlassPanel title="Scheduled operations" subtitle="Recurring power or snapshot actions">
                {schedulesLoading ? (
                  <Loader2 className="w-4 h-4 animate-spin text-[var(--text-muted)]" />
                ) : schedules.length === 0 ? (
                  <p className="text-sm text-[var(--text-muted)] mb-3">No schedules configured.</p>
                ) : (
                  <div className="overflow-x-auto">
                  <table className="w-full text-sm mb-3">
                    <thead><tr className="text-left text-xs text-[var(--text-muted)] border-b border-[var(--apple-hairline)]"><th scope="col" className="pb-1 pr-4">Action</th><th scope="col" className="pb-1 pr-4">Every</th><th scope="col" className="pb-1 pr-4">Next run</th><th scope="col" className="pb-1 pr-4">Label</th><th scope="col" /></tr></thead>
                    <tbody className="divide-y divide-[var(--apple-hairline)]/30">
                      {schedules.map((s) => (
                        <tr key={s.id}>
                          <td className="py-1 pr-4 font-mono">{s.action}</td>
                          <td className="py-1 pr-4 text-[var(--text-secondary)]">
                            {s.interval_minutes < 60
                              ? `${s.interval_minutes}m`
                              : s.interval_minutes < 1440
                              ? `${s.interval_minutes / 60}h`
                              : `${s.interval_minutes / 1440}d`}
                          </td>
                          <td className="py-1 pr-4 text-[var(--text-muted)] text-xs">{s.next_run_at ? new Date(s.next_run_at.replace(' ', 'T') + 'Z').toLocaleString() : '—'}</td>
                          <td className="py-1 pr-4 text-[var(--text-muted)]">{s.label || '—'}</td>
                          <td className="py-1 text-right">
                            <button
                              type="button"
                              className="p-1 hover:bg-red-600/20 rounded"
                              title="Delete schedule"
                              aria-label="Delete schedule"
                              onClick={async () => {
                                try { await deleteVmSchedule(id!, s.id); void loadSchedules() } catch (e: unknown) { toast.error(formatUserError(e)) }
                              }}
                            >
                              <Trash2 className={`w-3 h-3 ${statusToneClass('error')}`} />
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  </div>
                )}
                <div className="grid grid-cols-2 gap-2 text-sm mt-1">
                  <select aria-label="Action" className="input" value={newSchedAction} onChange={(e) => setNewSchedAction(e.target.value as VmScheduleAction)}>
                    <option value="start">Start</option>
                    <option value="shutdown">Graceful shutdown</option>
                    <option value="stop">Force stop</option>
                    <option value="snapshot">Snapshot</option>
                  </select>
                  <select aria-label="Interval" className="input" value={newSchedInterval} onChange={(e) => setNewSchedInterval(Number(e.target.value))}>
                    <option value={60}>Every hour</option>
                    <option value={360}>Every 6 hours</option>
                    <option value={720}>Every 12 hours</option>
                    <option value={1440}>Daily</option>
                    <option value={10080}>Weekly</option>
                  </select>
                  {newSchedAction === 'snapshot' && (
                    <div className="col-span-2 flex items-center gap-2">
                      <label className="text-xs text-[var(--text-muted)] whitespace-nowrap">Keep last</label>
                      <input aria-label="Keep last"
                        type="number"
                        className="input w-20 text-sm"
                        min={1}
                        max={100}
                        value={newSchedRetention ?? 5}
                        onChange={(e) => setNewSchedRetention(Number(e.target.value) || null)}
                      />
                      <label className="text-xs text-[var(--text-muted)]">snapshots</label>
                    </div>
                  )}
                  <input
                    aria-label="Label (optional)"
                    className="input col-span-2 text-sm"
                    placeholder="Label (optional)"
                    value={newSchedLabel}
                    onChange={(e) => setNewSchedLabel(e.target.value)}
                  />
                  <button
                    type="button"
                    className="btn-secondary col-span-2 text-sm"
                    disabled={newSchedSaving || vm.managed === false}
                    onClick={async () => {
                      setNewSchedSaving(true)
                      try {
                        await createVmSchedule(id!, {
                          action: newSchedAction,
                          interval_minutes: newSchedInterval,
                          retention: newSchedAction === 'snapshot' ? (newSchedRetention ?? null) : null,
                          label: newSchedLabel,
                        })
                        toast.success('Schedule created')
                        setNewSchedLabel('')
                        void loadSchedules()
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      } finally {
                        setNewSchedSaving(false)
                      }
                    }}
                  >
                    {newSchedSaving ? 'Saving…' : 'Add schedule'}
                  </button>
                </div>
              </MacGlassPanel>
              {vm.inventory_source !== 'kubevirt' && (
                <MacGlassPanel title="Domain XML" subtitle="Inline domain definition (libvirt define)">
                  <textarea
                    aria-label="Domain XML"
                    className="input w-full font-mono text-xs min-h-[12rem] mt-2"
                    value={domainXml}
                    onChange={(e) => setDomainXml(e.target.value)}
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    className="btn-secondary text-sm mt-3"
                    disabled={vm.managed === false || domainXmlSaving || !domainXml.trim()}
                    onClick={async () => {
                      setDomainXmlSaving(true)
                      try {
                        await putVmDomainXml(id!, domainXml)
                        toast.success('Domain XML updated')
                        await loadDomainXml()
                        await loadLibvirtDetails()
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      } finally {
                        setDomainXmlSaving(false)
                      }
                    }}
                  >
                    {domainXmlSaving ? 'Saving…' : 'Save XML (define)'}
                  </button>
                </MacGlassPanel>
              )}
              <MacGlassPanel title="High availability">
                <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={ha.enabled} onChange={(e) => setHa({ ...ha, enabled: e.target.checked })} /> Restart on host failure</label>
                <label className="flex items-center gap-2 text-sm mt-2"><input type="checkbox" checked={ha.fence_on_failure} onChange={(e) => setHa({ ...ha, fence_on_failure: e.target.checked })} /> Fence host on failure</label>
                <button type="button" className="btn-secondary text-sm mt-2" onClick={() => void act('HA policy updated', () => setVmHa(id, ha))}>Save HA policy</button>
              </MacGlassPanel>
              <MacGlassPanel title="Live migrate & clone" data-testid="vm-migrate-panel">
                <div className="grid gap-4 md:grid-cols-2">
                  <div>
                    <h3 className="font-semibold mb-2 flex items-center gap-2 text-sm"><MoveRight className="w-4 h-4" /> Live migrate</h3>
                    <select aria-label="Destination host" className="input w-full mb-2" value={destHost} onChange={(e) => setDestHost(e.target.value)}>
                      {hosts.map((h) => <option key={h.id} value={h.id}>{h.hostname}</option>)}
                    </select>
                    <div className="flex flex-wrap gap-3 mb-2 text-xs text-[var(--text-muted)]">
                      <label className="flex items-center gap-2">
                        <input type="checkbox" checked={migrateLive} onChange={(e) => setMigrateLive(e.target.checked)} /> Live migration
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" checked={migratePostcopy} onChange={(e) => setMigratePostcopy(e.target.checked)} /> Post-copy
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" checked={migrateUndefineSource} onChange={(e) => setMigrateUndefineSource(e.target.checked)} /> Undefine source (permanent)
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" checked={migrateTunnelled} onChange={(e) => setMigrateTunnelled(e.target.checked)} /> Tunnelled
                      </label>
                      <label className="flex items-center gap-2">
                        <input type="checkbox" checked={migrateCopyStorage} onChange={(e) => setMigrateCopyStorage(e.target.checked)} /> Copy disk storage (non-shared)
                      </label>
                      <label className="flex items-center gap-2 sm:col-span-2">
                        Disk paths (comma-separated, optional)
                        <input
                          className="input flex-1 text-xs font-mono"
                          placeholder="/var/lib/libvirt/images/vm.qcow2"
                          value={migrateDisks}
                          onChange={(e) => setMigrateDisks(e.target.value)}
                        />
                      </label>
                      <label className="flex items-center gap-2 sm:col-span-2">
                        Destination storage URI (optional)
                        <input
                          className="input flex-1 text-xs font-mono"
                          placeholder="qemu+ssh://dest/system"
                          value={migrateDisksUri}
                          onChange={(e) => setMigrateDisksUri(e.target.value)}
                        />
                      </label>
                      {migrationDisks.length > 0 ? (
                        <div className="sm:col-span-2 flex flex-wrap gap-2">
                          {migrationDisks.map((d) => (
                            <button
                              key={d.target}
                              type="button"
                              className="btn-secondary text-xs font-mono"
                              onClick={() => {
                                const path = d.source
                                setMigrateDisks((prev) => {
                                  const parts = prev.split(',').map((s) => s.trim()).filter(Boolean)
                                  if (parts.includes(path)) return prev
                                  return [...parts, path].join(', ')
                                })
                              }}
                            >
                              + {d.target}
                            </button>
                          ))}
                        </div>
                      ) : null}
                    </div>
                    <div className="flex gap-2">
                      <button type="button" className="btn-secondary text-sm" disabled={!destHost} onClick={async () => {
                        try {
                          setPrecheck(await migratePrecheck(id, destHost, migrateLive))
                        } catch (e: unknown) { toast.error(formatUserError(e)) }
                      }}>Pre-check</button>
                      <button
                        type="button"
                        className="btn-secondary text-sm"
                        disabled={!destHost}
                        onClick={() => void act('Migration queued', () => vmMigrate(id, {
                          dest_host_id: destHost,
                          live: migrateLive,
                          bandwidth_mib: migrateBandwidth && !Number.isNaN(Number(migrateBandwidth)) ? Number(migrateBandwidth) : undefined,
                          postcopy: migratePostcopy,
                          undefine_source: migrateUndefineSource,
                          tunnelled: migrateTunnelled,
                          migrate_disks: migrateDisks.split(',').map((s) => s.trim()).filter(Boolean),
                          disks_uri: migrateDisksUri.trim() || undefined,
                          copy_storage: migrateCopyStorage,
                        }))}
                      >
                        Migrate
                      </button>
                    </div>
                    {precheck && (
                      <ul className="text-xs mt-2 space-y-1">{(precheck.checks ?? []).map((c) => (
                        <li key={c.name} className={statusToneClass(c.passed ? 'ok' : 'error')}>
                          {c.name}: {c.message}
                          {c.remediation && !c.passed && (
                            <p className="text-[var(--text-muted)] pl-2 mt-1">
                              Fix: {c.remediation}
                              {!c.passed && c.name.toLowerCase().includes('network') && (
                                <button type="button" className="btn-secondary text-xs ml-2" onClick={() => setTab('network')}>Choose network</button>
                              )}
                            </p>
                          )}
                        </li>
                      ))}</ul>
                    )}
                  </div>
                  <div>
                    <h3 className="font-semibold mb-2 flex items-center gap-2 text-sm"><Copy className="w-4 h-4" /> Clone</h3>
                    <input aria-label="Clone name" className="input w-full mb-2" placeholder="new-vm-name" value={cloneName} onChange={(e) => setCloneName(e.target.value)} />
                    <select aria-label="Clone mode" className="input w-full mb-2 text-sm" value={cloneMode} onChange={(e) => setCloneMode(e.target.value as 'linked' | 'full')}>
                      <option value="linked">Linked clone (thin)</option>
                      <option value="full">Full clone (independent disk)</option>
                    </select>
                    <button type="button" className="btn-secondary text-sm" disabled={!cloneName} onClick={() => void act('Clone queued', () => vmClone(id, cloneName, cloneMode))}>Clone</button>
                  </div>
                </div>
              </MacGlassPanel>
              <MacGlassPanel title="Lifecycle & export">
                <p className="text-xs text-[var(--text-muted)] mb-3">
                  Retire stops the VM, tags it, and blocks start. Export disk queues a full qcow2 backup task.
                </p>
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    disabled={vm.lifecycle_phase === 'retired'}
                    onClick={() => setVmConfirmOp('retire')}
                  >
                    Retire VM
                  </button>
                  <button type="button" className="btn-secondary text-sm" onClick={() => void act('Disk export queued', () => exportVmDisk(id))}>
                    Export disk (qcow2)
                  </button>
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    onClick={async () => {
                      try {
                        setIacBundle(await exportVmIac(id))
                        toast.success('IaC bundle loaded — download below')
                      } catch (e: unknown) {
                        toast.error(formatUserError(e))
                      }
                    }}
                  >
                    Load IaC export
                  </button>
                </div>
                {iacBundle && (
                  <div className="mt-3 space-y-2 text-xs">
                    <button type="button" className="btn-secondary text-xs" onClick={() => void downloadVmIacZip(id, vm.name)}>
                      Download ZIP (IaC)
                    </button>
                    <button type="button" className="btn-secondary text-xs" onClick={() => downloadVmIacBundle(iacBundle)}>
                      Download JSON bundle
                    </button>
                    {(['terraform', 'ansible_role', 'cloud_init', 'domain_xml'] as const).map((key) => (
                      <button
                        key={key}
                        type="button"
                        className="btn-secondary text-xs mr-2"
                        onClick={() => {
                          const blob = new Blob([iacBundle[key]], { type: 'text/plain' })
                          const a = document.createElement('a')
                          a.href = URL.createObjectURL(blob)
                          a.download = `${vm.name}-${key}.${key === 'terraform' ? 'tf' : key === 'domain_xml' ? 'xml' : 'txt'}`
                          a.click()
                          URL.revokeObjectURL(a.href)
                        }}
                      >
                        Download {key}
                      </button>
                    ))}
                  </div>
                )}
              </MacGlassPanel>
              {vm.inventory_source !== 'kubevirt' && (
                <MacGlassPanel title="Publish golden template">
                  <input aria-label="Template name" className="input w-full mb-2 text-sm" placeholder="template-name" value={publishTplName} onChange={(e) => setPublishTplName(e.target.value)} />
                  <input aria-label="Template version" className="input w-full mb-2 text-sm" placeholder="version" value={publishTplVersion} onChange={(e) => setPublishTplVersion(e.target.value)} />
                  <button
                    type="button"
                    className="btn-secondary text-sm"
                    disabled={!publishTplName.trim()}
                    onClick={() =>
                      void act('Template published', () =>
                        publishVmAsTemplate(id, {
                          template_name: publishTplName.trim(),
                          version: publishTplVersion.trim() || '1.0.0',
                          marketplace: true,
                        }),
                      )
                    }
                  >
                    Publish from VM
                  </button>
                </MacGlassPanel>
              )}
              <MacGlassPanel title="VM spec">
                {specData ? <JsonInspector data={specData} /> : <p className="text-sm text-[var(--text-muted)]">Spec unavailable</p>}
              </MacGlassPanel>
            </div>
          )}

          {tab === 'advanced' && vm.inventory_source !== 'kubevirt' && (
            <PlatformVmAdvanced
              vmId={id!}
              hostId={vm.host_id}
              vmName={vm.name}
              managed={vm.managed}
              libvirtDetails={libvirtDetails}
              onChanged={() => void loadLibvirtDetails()}
            />
          )}

          {vm.inventory_source === 'kubevirt' && id ? (
            <VmKubevirtHardwareDrawer
              open={hardwareDrawerOpen}
              onClose={() => setHardwareDrawerOpen(false)}
              vmId={id}
              vmName={vm.name}
              hardware={kubevirtHardware}
            />
          ) : null}

          {vm.inventory_source !== 'kubevirt' && id && (
            <>
              <VmHardwareDrawer
                open={hardwareDrawerOpen}
                onClose={() => setHardwareDrawerOpen(false)}
                vmId={id}
                vmName={vm.name}
                hostId={vm.host_id}
                managed={vm.managed}
                vmState={vm.observed_state}
                hardware={hardware}
                portForwardRules={portForwardRules}
                protocols={consolePlan?.protocols ?? []}
                canBrowseHost={canBrowseHost}
                onPlanRefresh={() => void loadPortForwards()}
              />
              <VmEditHardwareDrawer
                open={hardwareEditOpen}
                onClose={() => setHardwareEditOpen(false)}
                vmId={id}
                vmName={vm.name}
                hostId={vm.host_id}
                managed={vm.managed}
                vmState={vm.observed_state}
                hardware={hardware}
                canBrowseHost={canBrowseHost}
              />
              <VmCpuTopologyModal
                open={cpuModalOpen}
                vmId={id}
                vmName={vm.name}
                onClose={() => setCpuModalOpen(false)}
                onSaved={() => {
                  void loadComputeTopology()
                  void loadLibvirtDetails()
                  void load()
                }}
                onNotify={(m) => toast.success(m)}
                onError={(m) => toast.error(m)}
              />
              <VmMemorySizingModal
                open={memoryModalOpen}
                vmId={id}
                vmName={vm.name}
                running={vm.observed_state === 'running'}
                onClose={() => setMemoryModalOpen(false)}
                onSaved={() => {
                  void loadComputeTopology()
                  void loadLibvirtDetails()
                  void load()
                }}
                onNotify={(m) => toast.success(m)}
                onError={(m) => toast.error(m)}
              />
            </>
          )}

          {vm.inventory_source !== 'kubevirt' && (
            <>
              <BrowseHostPathModal
                open={isoBrowseOpen}
                onClose={() => setIsoBrowseOpen(false)}
                title="Browse for ISO"
                canSelectFile={isIsoFileName}
                onSelectPath={(p) => setIsoPath(p)}
              />
              <BrowseHostPathModal
                open={attachDiskBrowseOpen}
                onClose={() => setAttachDiskBrowseOpen(false)}
                title="Browse for disk image"
                canSelectFile={isHostDiskImageFileName}
                onSelectPath={(p) => setAttachPath(p)}
              />
            </>
          )}
        </>
      )}
      <ConfirmDialog
        open={vmConfirmOp === 'delete'}
        title="Delete VM Permanently"
        message="Delete this VM permanently? All disks and configuration will be removed. This cannot be undone."
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setVmConfirmOp(null)}
        onConfirm={() => { setVmConfirmOp(null); void queueVmDelete('Delete queued') }}
      />
      <ConfirmDialog
        open={vmConfirmOp === 'delete_kubevirt'}
        title="Delete KubeVirt VM"
        message={`Delete KubeVirt VirtualMachine ${vm?.k8s_namespace ?? 'default'}/${vm?.name ?? ''}? The CR will be removed from the cluster.`}
        confirmLabel="Delete"
        variant="danger"
        onCancel={() => setVmConfirmOp(null)}
        onConfirm={() => { setVmConfirmOp(null); if (vm) void act('Delete requested', () => deleteK8sKubevirtVm(vm.k8s_namespace ?? 'default', vm.name)) }}
      />
      <ConfirmDialog
        open={vmConfirmOp === 'remove_stale'}
        title="Remove Stale VM Record"
        message="Remove this stale VM record from the platform? The VM no longer exists in the hypervisor inventory."
        confirmLabel="Remove"
        variant="danger"
        onCancel={() => setVmConfirmOp(null)}
        onConfirm={() => { setVmConfirmOp(null); void queueVmDelete('Stale VM removed') }}
      />
      <ConfirmDialog
        open={vmConfirmOp === 'retire'}
        title="Retire VM"
        message="Retire this VM? It will be stopped and cannot be started until restored manually."
        confirmLabel="Retire"
        variant="warning"
        onCancel={() => setVmConfirmOp(null)}
        onConfirm={() => { setVmConfirmOp(null); if (id) void act('VM retired', () => retirePlatformVm(id, true)) }}
      />
      <ConfirmDialog
        open={snapDestructiveConfirm !== null}
        title={snapDestructiveConfirm?.action === 'delete' ? 'Delete Snapshot' : 'Revert Snapshot'}
        message={snapDestructiveConfirm?.action === 'delete'
          ? `Permanently delete snapshot '${snapDestructiveConfirm?.snapName}'? This cannot be undone.`
          : `Revert this VM to snapshot '${snapDestructiveConfirm?.snapName}'? The guest's current disk and memory state will be discarded.`}
        confirmLabel={snapDestructiveConfirm?.action === 'delete' ? 'Delete' : 'Revert'}
        variant={snapDestructiveConfirm?.action === 'delete' ? 'danger' : 'warning'}
        onCancel={() => setSnapDestructiveConfirm(null)}
        onConfirm={() => {
          const c = snapDestructiveConfirm
          setSnapDestructiveConfirm(null)
          if (c) void runSnapshotAction(c.snapName, c.action, c.run, c.label)
        }}
      />
      <ConfirmDialog
        open={snapConfirmMsg !== null}
        title={`Confirm ${snapConfirmMsg?.action ?? 'Action'}`}
        message={`${snapConfirmMsg?.message ?? ''}\n\nContinue with ${snapConfirmMsg?.action ?? 'this action'}?`}
        confirmLabel="Continue"
        variant="warning"
        onCancel={() => { setSnapConfirmMsg(null); snapActionRef.current = null }}
        onConfirm={async () => {
          setSnapConfirmMsg(null)
          if (snapActionRef.current) await snapActionRef.current()
          snapActionRef.current = null
        }}
      />
    </PageLayout>
  )
}

function InfoCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="glass glass-elevated rounded-liquid p-3 glass-hover-lift">
      <p className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">{label}</p>
      <p className="font-semibold text-[var(--text-primary)] mt-1 capitalize tracking-tight">{value}</p>
    </div>
  )
}
