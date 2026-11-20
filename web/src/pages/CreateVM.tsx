// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useEffect, useRef, useCallback, useMemo } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { startPackerGoldenBuildJob, streamJobLogs } from '../api/jobs'
import { createVMWithProgress, CreateVmRequest, getTemplates, VmTemplate, vmDetailRoute } from '../api/vm'
import { getLibvirtSummary } from '../api/host'
import { listNetworks, NetworkInfo } from '../api/network'
import { generateCloudInit, listIsos, listSavedTemplates, ImageFile } from '../api/extras'
import { listPools, listVolumes, StoragePoolInfo, StorageVolumeInfo } from '../api/storage'
import { guestOsDetect, guestOsList, guestRhelImageUrl, type GuestOsRow } from '../api/guestImages'
import {
  createVmDefaultsStorageKey,
  loadCreateVmDefaults,
  saveCreateVmDefaults,
  type CreateVmDefaultsPayload,
} from '../utils/createVmDefaults'
import { getServerCreateVmDefaults, putServerCreateVmDefaults } from '../api/system'
import { getSession, type SessionRole } from '../api/auth'
import { parseOsinfoDetectVariant } from '../utils/osinfoDetect'
import { BrowseHostPathModal, isHostDiskImageFileName, isIsoFileName } from '../components/BrowseHostPathModal'
import { BuildStepTimeline } from '../components/BuildStepTimeline'
import WizardStepper from '../components/WizardStepper'
import { ChoiceCard, ChoiceCardGrid } from '../components/ChoiceCards'
import { useToastContext } from '../contexts/ToastContext'
import { usePlatformInfo } from '../contexts/PlatformInfoContext'
import FluxvmCreatePanel from '../components/vm/FluxvmCreatePanel'
import {
  MACHINA_PACKER_SCRIPT_GUESTS,
  PACKER_SCRIPT_REPO,
  PACKER_SCRIPT_SYSTEM,
  PACKER_WINDOWS_DOCKUR_REPO,
  PACKER_WINDOWS_DOCKUR_SYSTEM,
} from '../data/packerGuests'
import {
  computeGoldenForgeTimeline,
  computeVmCreateTimeline,
  GOLDEN_FORGE_TIMELINE_LABELS,
  VM_CREATE_TIMELINE_LABELS,
} from '../utils/buildProgress'
import {
  ArrowLeft,
  Boxes,
  Disc,
  Download,
  FolderOpen,
  Globe,
  HardDrive,
  Layers,
  LayoutTemplate,
  Link2,
  Monitor,
  Network,
  Terminal,
  Zap,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { formatUserError } from '../utils/apiError'
import PageLayout from '../components/PageLayout'
import { libvirtErrorHints } from '../utils/libvirtHints'
import { statusToneClass } from '../utils/semanticColors'

type InstallSource = 'iso' | 'url' | 'pxe' | 'download'
type StorageMode = 'new' | 'volume'
type PageFlow = 'install' | 'golden' | 'fluxvm'
type GoldenKind = 'template' | 'backing'
type GuestProfile = 'auto' | 'linux' | 'windows'

const INSTALL_WIZARD_STEPS = ['Source & OS', 'Disk', 'Network & display', 'Cloud-init', 'Review'] as const

const LINUX_PACKER_GUESTS = MACHINA_PACKER_SCRIPT_GUESTS.filter((g) => g.family !== 'windows')
const WINDOWS_PACKER_GUESTS = MACHINA_PACKER_SCRIPT_GUESTS.filter((g) => g.family === 'windows')

function formatSummaryMemory(mb: number): string {
  if (!Number.isFinite(mb) || mb <= 0) return '—'
  return mb >= 1024 ? `${(mb / 1024).toFixed(mb % 1024 ? 1 : 0)} GiB` : `${mb} MiB`
}

export default function CreateVMPage() {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const toast = useToastContext()
  const { info: platformInfo } = usePlatformInfo()
  const fluxvmEnabled = platformInfo?.fluxvm?.enabled === true
  const logEndRef = useRef<HTMLDivElement>(null)
  const packerLogEndRef = useRef<HTMLDivElement>(null)

  const [installSource, setInstallSource] = useState<InstallSource>('iso')
  const [vmName, setVmName] = useState('')
  const [vcpus, setVcpus] = useState(2)
  const [memoryMb, setMemoryMb] = useState(2048)
  const [diskGb, setDiskGb] = useState(20)
  const [network, setNetwork] = useState('default')
  const [firmware, setFirmware] = useState('bios')
  const [secureBoot, setSecureBoot] = useState(false)
  const [vtpm, setVtpm] = useState(false)
  const [osVariant, setOsVariant] = useState('')
  const [iso, setIso] = useState('')
  const [guestProfile, setGuestProfile] = useState<GuestProfile>('auto')
  const [virtioWinIso, setVirtioWinIso] = useState('')
  const [virtInstallLocation, setVirtInstallLocation] = useState('')
  const [virtInstallInstallOs, setVirtInstallInstallOs] = useState('')
  const [virtInstallExtraArgs, setVirtInstallExtraArgs] = useState('')
  const [virtInstallPxeNetwork, setVirtInstallPxeNetwork] = useState('')
  const [cloudInitIso, setCloudInitIso] = useState('')
  const [cloudInitUser, setCloudInitUser] = useState('')
  const [cloudInitPassword, setCloudInitPassword] = useState('')
  const [cloudInitSshKey, setCloudInitSshKey] = useState('')
  const [pathCheckOff, setPathCheckOff] = useState(false)

  const [storageMode, setStorageMode] = useState<StorageMode>('new')
  const [pools, setPools] = useState<StoragePoolInfo[]>([])
  const [diskPool, setDiskPool] = useState('')
  const [volumes, setVolumes] = useState<StorageVolumeInfo[]>([])
  const [diskVol, setDiskVol] = useState('')

  const [graphicsType, setGraphicsType] = useState<'vnc' | 'spice'>('vnc')
  const [graphicsListen, setGraphicsListen] = useState('127.0.0.1')

  const [networks, setNetworks] = useState<NetworkInfo[]>([])
  const [isoScan, setIsoScan] = useState<ImageFile[]>([])
  const [catalogWarning, setCatalogWarning] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [createLog, setCreateLog] = useState<string[]>([])
  const [isoBrowseOpen, setIsoBrowseOpen] = useState(false)
  const [cloudBrowseOpen, setCloudBrowseOpen] = useState(false)
  const [virtioBrowseOpen, setVirtioBrowseOpen] = useState(false)

  const [pageFlow, setPageFlow] = useState<PageFlow>('install')
  const [goldenKind, setGoldenKind] = useState<GoldenKind>('template')
  const [savedTemplates, setSavedTemplates] = useState<VmTemplate[]>([])
  const [selectedTemplateName, setSelectedTemplateName] = useState('')
  const [templateDiskMode, setTemplateDiskMode] = useState<'backing' | 'copy'>('backing')
  const [backingGoldenPath, setBackingGoldenPath] = useState('')
  const [goldenOverlayGb, setGoldenOverlayGb] = useState(40)
  const [backingBrowseOpen, setBackingBrowseOpen] = useState(false)

  const [packerGuestId, setPackerGuestId] = useState(MACHINA_PACKER_SCRIPT_GUESTS[0]?.id ?? '')
  const [packerRunning, setPackerRunning] = useState(false)

  const [libSummary, setLibSummary] = useState<Awaited<ReturnType<typeof getLibvirtSummary>> | null>(null)
  /** When daemon has dual libvirt: where to define the new domain. */
  const [createLibvirtTarget, setCreateLibvirtTarget] = useState<'default' | 'system' | 'session'>('default')
  const [packerLog, setPackerLog] = useState<string[]>([])
  const [createProgressOk, setCreateProgressOk] = useState(false)
  const [createProgressFailed, setCreateProgressFailed] = useState(false)
  const [packerProgressOk, setPackerProgressOk] = useState(false)
  const [packerProgressFailed, setPackerProgressFailed] = useState(false)

  const [useInstallWizard, setUseInstallWizard] = useState(true)
  const [installWizardStep, setInstallWizardStep] = useState(0)
  const [osDetectBusy, setOsDetectBusy] = useState(false)

  useEffect(() => {
    if (!useInstallWizard || pageFlow !== 'install') return
    document.getElementById(`create-vm-step-${installWizardStep}`)?.scrollIntoView({
      behavior: 'smooth',
      block: 'start',
    })
  }, [installWizardStep, useInstallWizard, pageFlow])

  const createDefaultsKey = useMemo(() => {
    const host = typeof window !== 'undefined' ? window.location.hostname : ''
    return createVmDefaultsStorageKey(libSummary?.configured_uri, host)
  }, [libSummary?.configured_uri])

  const [sessionRole, setSessionRole] = useState<SessionRole | null>(null)
  const [guestOsRows, setGuestOsRows] = useState<GuestOsRow[]>([])
  const [libvirtTemplates, setLibvirtTemplates] = useState<VmTemplate[]>([])
  const [cloudInitGenBusy, setCloudInitGenBusy] = useState(false)
  const [rhelAccessToken, setRhelAccessToken] = useState('')
  const [rhelImageBusy, setRhelImageBusy] = useState(false)
  const [rhelImageHint, setRhelImageHint] = useState<string | null>(null)

  const vmCreateTimeline = useMemo(
    () => computeVmCreateTimeline(createLog, submitting, createProgressOk, createProgressFailed),
    [createLog, submitting, createProgressOk, createProgressFailed],
  )

  const goldenForgeTimeline = useMemo(
    () => computeGoldenForgeTimeline(packerLog, packerRunning, packerProgressOk, packerProgressFailed),
    [packerLog, packerRunning, packerProgressOk, packerProgressFailed],
  )

  const loadVolumes = useCallback(async (pool: string) => {
    if (!pool) {
      setVolumes([])
      setDiskVol('')
      return
    }
    try {
      const v = await listVolumes(pool)
      setVolumes(v)
      setDiskVol((cur) => (v.some((x) => x.name === cur) ? cur : ''))
    } catch {
      setVolumes([])
      setDiskVol('')
    }
  }, [])

  useEffect(() => {
    getLibvirtSummary().then(setLibSummary).catch(() => setLibSummary(null))
  }, [])

  useEffect(() => {
    void (async () => {
      const warnings: string[] = []
      const [netR, isoR, poolR] = await Promise.allSettled([
        listNetworks(),
        listIsos(),
        listPools(),
      ])
      if (netR.status === 'fulfilled') {
        setNetworks(Array.isArray(netR.value) ? netR.value : [])
      } else {
        warnings.push(`Networks: ${formatUserError(netR.reason)}`)
        setNetworks([])
      }
      if (isoR.status === 'fulfilled') {
        setIsoScan(isoR.value.files ?? [])
      } else {
        warnings.push(`ISO scan: ${formatUserError(isoR.reason)}`)
        setIsoScan([])
      }
      if (poolR.status === 'fulfilled') {
        const poolList = Array.isArray(poolR.value) ? poolR.value : []
        setPools(poolList)
        setDiskPool((prev) => prev || (poolList[0]?.name ?? ''))
      } else {
        warnings.push(`Storage pools: ${formatUserError(poolR.reason)}`)
        setPools([])
      }
      setCatalogWarning(warnings.length > 0 ? warnings.join(' · ') : null)
    })()
  }, [])

  useEffect(() => {
    if (storageMode === 'volume' && diskPool) void loadVolumes(diskPool)
  }, [storageMode, diskPool, loadVolumes])

  useEffect(() => {
    if (createLog.length) logEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [createLog])

  useEffect(() => {
    if (packerLog.length) packerLogEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [packerLog])

  useEffect(() => {
    if (pageFlow !== 'golden') return
    listSavedTemplates()
      .then((t) => {
        const templates = Array.isArray(t) ? t : []
        setSavedTemplates(templates)
        setSelectedTemplateName((prev) => {
          if (prev && templates.some((x) => x.name === prev)) return prev
          const withGolden = templates.filter((x) => x.base_image)
          return withGolden[0]?.name ?? templates[0]?.name ?? ''
        })
      })
      .catch(() => setSavedTemplates([]))
  }, [pageFlow])

  useEffect(() => {
    getSession()
      .then((s) => {
        if (s.authenticated) setSessionRole(s.role ?? 'admin')
        else setSessionRole(null)
      })
      .catch(() => setSessionRole(null))
  }, [])

  useEffect(() => {
    guestOsList()
      .then((r) => setGuestOsRows(r.oses ?? []))
      .catch(() => setGuestOsRows([]))
  }, [])

  useEffect(() => {
    getTemplates()
      .then((t) => setLibvirtTemplates(Array.isArray(t) ? t : []))
      .catch(() => setLibvirtTemplates([]))
  }, [])

  useEffect(() => {
    const key = createDefaultsKey
    let cancelled = false
    ;(async () => {
      let server: Record<string, unknown> = {}
      try {
        server = await getServerCreateVmDefaults()
      } catch {
        /* daemon offline or endpoint missing */
      }
      if (cancelled) return
      const localRaw = loadCreateVmDefaults(key) ?? {}
      const local = Object.fromEntries(
        Object.entries(localRaw).filter(([, v]) => v !== undefined),
      ) as Record<string, unknown>
      const d: Record<string, unknown> = { ...server, ...local }
      if (typeof d.vcpus === 'number') setVcpus(d.vcpus)
      if (typeof d.memory_mb === 'number') setMemoryMb(d.memory_mb)
      if (typeof d.disk_gb === 'number') setDiskGb(d.disk_gb)
      if (typeof d.network === 'string' && d.network) setNetwork(d.network)
      if (typeof d.firmware === 'string' && d.firmware) setFirmware(d.firmware)
      if (d.graphics_type === 'vnc' || d.graphics_type === 'spice') setGraphicsType(d.graphics_type)
      if (typeof d.graphics_listen === 'string' && d.graphics_listen) setGraphicsListen(d.graphics_listen)
      if (d.guest_profile === 'auto' || d.guest_profile === 'linux' || d.guest_profile === 'windows') {
        setGuestProfile(d.guest_profile)
      }
      if (typeof d.os_variant === 'string') setOsVariant(d.os_variant)
      if (typeof d.virt_install_extra_args === 'string') setVirtInstallExtraArgs(d.virt_install_extra_args)
      if (d.install_source === 'iso' || d.install_source === 'url' || d.install_source === 'pxe' || d.install_source === 'download') {
        setInstallSource(d.install_source)
      }
      if (d.storage_mode === 'new' || d.storage_mode === 'volume') setStorageMode(d.storage_mode)
      if (typeof d.disk_pool === 'string' && d.disk_pool) setDiskPool(d.disk_pool)
      if (typeof d.cloud_init_user === 'string') setCloudInitUser(d.cloud_init_user)
      if (typeof d.cloud_init_ssh_pubkey === 'string') setCloudInitSshKey(d.cloud_init_ssh_pubkey)
      if (typeof d.virtio_win_iso === 'string') setVirtioWinIso(d.virtio_win_iso)
    })()
    return () => {
      cancelled = true
    }
  }, [createDefaultsKey])

  useEffect(() => {
    const isoPath = searchParams.get('iso_path')
    if (isoPath) {
      setPageFlow('install')
      setInstallSource('iso')
      setIso(isoPath)
    }
    const preName = searchParams.get('name')
    if (preName) setVmName(preName)
    const preVcpus = searchParams.get('vcpus')
    if (preVcpus) {
      const n = parseInt(preVcpus, 10)
      if (Number.isFinite(n) && n > 0) setVcpus(n)
    }
    const preMem = searchParams.get('memory_mb')
    if (preMem) {
      const n = parseInt(preMem, 10)
      if (Number.isFinite(n) && n > 0) setMemoryMb(n)
    }
    const preDisk = searchParams.get('disk_gb')
    if (preDisk) {
      const n = parseInt(preDisk, 10)
      if (Number.isFinite(n) && n > 0) setDiskGb(n)
    }
  }, [searchParams])

  const setSource = (src: InstallSource) => {
    setInstallSource(src)
    if (src !== 'iso') setIso('')
    if (src !== 'url') setVirtInstallLocation('')
    if (src === 'download') {
      const first = MACHINA_PACKER_SCRIPT_GUESTS[0]
      if (first) {
        setVirtInstallInstallOs(first.virtInstallDownloadOs)
        setOsVariant(first.osVariantHint)
      }
    } else {
      setVirtInstallInstallOs('')
    }
  }

  useEffect(() => {
    if (guestProfile !== 'windows') return
    // Windows installs generally feel best with SPICE/QXL and UEFI.
    setGraphicsType('spice')
    setFirmware('uefi')
  }, [guestProfile])

  const runPackerGoldenBuild = async () => {
    const gid = packerGuestId.trim()
    if (!gid) {
      toast.warning('Choose a golden image profile')
      return
    }
    setPackerProgressOk(false)
    setPackerProgressFailed(false)
    setPackerRunning(true)
    setPackerLog([`[machina] Starting Packer build for “${gid}”…`])
    try {
      const { id } = await startPackerGoldenBuildJob({ guest: gid })
      setPackerLog((prev) => [...prev, `[machina] Job ${id} — live output:`])
      await streamJobLogs(id, {
        onLogChunk: (chunk) => {
          const lines = chunk.split('\n').filter((l) => l.length > 0)
          if (lines.length) setPackerLog((prev) => [...prev, ...lines])
        },
        onComplete: (data) => {
          try {
            const j = JSON.parse(data) as { path?: string; status?: string }
            if (j?.path) {
              setPackerLog((prev) => [...prev, `[machina] Artifact: ${j.path}`])
            }
          } catch {
            /* ignore */
          }
          setPackerProgressOk(true)
          toast.success(
            WINDOWS_PACKER_GUESTS.some((g) => g.id === gid)
              ? 'Windows golden ready — template registered; clone from golden or export to KubeVirt'
              : 'Golden image build finished — see Jobs for the qcow2 path',
          )
          setPackerRunning(false)
        },
        onError: (msg) => {
          setPackerProgressFailed(true)
          toast.error(msg)
          setPackerRunning(false)
        },
      })
    } catch (e: unknown) {
      setPackerProgressFailed(true)
      toast.error(formatUserError(e))
      setPackerRunning(false)
    }
  }

  const canBrowseHost = sessionRole === 'admin'

  const buildDefaultsPayload = (): Record<string, unknown> => ({
    vcpus,
    memory_mb: memoryMb,
    disk_gb: diskGb,
    network,
    firmware,
    graphics_type: graphicsType,
    graphics_listen: graphicsListen,
    guest_profile: guestProfile,
    os_variant: osVariant.trim() || undefined,
    virt_install_extra_args: virtInstallExtraArgs.trim() || undefined,
    install_source: installSource,
    storage_mode: storageMode,
    disk_pool: diskPool || undefined,
    cloud_init_user: cloudInitUser.trim() || undefined,
    cloud_init_ssh_pubkey: cloudInitSshKey.trim() || undefined,
    virtio_win_iso: virtioWinIso.trim() || undefined,
  })

  const saveBrowserDefaults = () => {
    try {
      saveCreateVmDefaults(createDefaultsKey, buildDefaultsPayload() as Partial<CreateVmDefaultsPayload>)
      toast.success('Defaults saved in this browser')
    } catch {
      toast.error('Could not save defaults')
    }
  }

  const saveServerDefaults = async () => {
    if (sessionRole !== 'admin' && sessionRole !== 'operator') {
      toast.warning('Saving hypervisor defaults requires operator or admin')
      return
    }
    try {
      await putServerCreateVmDefaults(buildDefaultsPayload())
      toast.success('Defaults saved on hypervisor')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    }
  }

  const runOsDetect = async () => {
    const url = virtInstallLocation.trim()
    if (!url) {
      toast.warning('Enter an install tree URL first')
      return
    }
    setOsDetectBusy(true)
    try {
      const r = await guestOsDetect(url)
      if (r.exit_code === 0 && r.stdout) {
        const parsed = parseOsinfoDetectVariant(r.stdout)
        if (parsed) {
          setOsVariant(parsed)
          toast.success(`osinfo-detect: applied “${parsed}” (verify against libosinfo)`)
        } else {
          toast.warning('Could not parse osinfo-detect stdout — tools missing or unknown format')
        }
      } else {
        toast.warning(r.stderr || 'osinfo-detect failed — install osinfo-tools on the hypervisor')
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setOsDetectBusy(false)
    }
  }

  const handleCreate = async () => {
    const name = vmName.trim()
    if (!name) {
      toast.warning('Name is required')
      return
    }

    if (storageMode === 'volume') {
      if (!diskPool.trim() || !diskVol.trim()) {
        toast.warning('Choose a storage pool and volume, or switch to “Create new disk image”.')
        return
      }
    }

    if (installSource === 'iso' && !iso.trim()) {
      toast.warning('Select or enter the install ISO path on the hypervisor')
      return
    }
    if (installSource === 'url' && !virtInstallLocation.trim()) {
      toast.warning('Enter the install URL or tree (virt-install --location), e.g. https://…/os/')
      return
    }
    if (installSource === 'download' && !virtInstallInstallOs.trim()) {
      toast.warning('Choose an OS for automatic download')
      return
    }

    const req: CreateVmRequest = {
      name,
      vcpus,
      memory_mb: memoryMb,
      disk_gb: diskGb,
      network,
      firmware,
      secure_boot: secureBoot && firmware === 'uefi' ? true : undefined,
      vtpm: vtpm ? true : undefined,
      create_backend: 'virt_install',
      os_variant: osVariant.trim() || undefined,
      guest_profile: guestProfile !== 'auto' ? guestProfile : undefined,
      virtio_win_iso: virtioWinIso.trim() || undefined,
      graphics_type: graphicsType,
      graphics_listen: graphicsListen.trim() || undefined,
      cloud_init_iso: cloudInitIso.trim() || undefined,
      cloud_init_user: cloudInitUser.trim() || undefined,
      cloud_init_password: cloudInitPassword || undefined,
      cloud_init_ssh_pubkey: cloudInitSshKey.trim() || undefined,
      virt_install_path_in_use_check_off: pathCheckOff || undefined,
    }
    if (installSource !== 'pxe' && virtInstallExtraArgs.trim()) {
      req.virt_install_extra_args = virtInstallExtraArgs.trim()
    }

    if (libSummary?.dual_connection) {
      if (createLibvirtTarget === 'session') req.libvirt_connection = 'session'
      else if (createLibvirtTarget === 'system') req.libvirt_connection = 'system'
    }

    if (storageMode === 'volume') {
      req.root_disk_storage_pool = diskPool.trim()
      req.root_disk_storage_volume = diskVol.trim()
    }

    if (installSource === 'iso') {
      req.iso = iso.trim()
    } else if (installSource === 'url') {
      req.virt_install_location = virtInstallLocation.trim()
    } else if (installSource === 'pxe') {
      req.virt_install_pxe = true
      if (virtInstallPxeNetwork.trim()) req.virt_install_pxe_network = virtInstallPxeNetwork.trim()
    } else {
      req.virt_install_install_os = virtInstallInstallOs.trim()
    }

    setSubmitting(true)
    setCreateProgressOk(false)
    setCreateProgressFailed(false)
    setCreateLog([])
    try {
      await createVMWithProgress(req, (line) => setCreateLog((prev) => [...prev, line]))
      setCreateProgressOk(true)
      saveCreateVmDefaults(createDefaultsKey, {
        vcpus,
        memory_mb: memoryMb,
        disk_gb: diskGb,
        network,
        firmware,
        graphics_type: graphicsType,
        graphics_listen: graphicsListen,
        guest_profile: guestProfile,
        os_variant: osVariant.trim() || undefined,
        virt_install_extra_args: virtInstallExtraArgs.trim() || undefined,
        install_source: installSource,
        storage_mode: storageMode,
        disk_pool: diskPool || undefined,
        cloud_init_user: cloudInitUser.trim() || undefined,
        cloud_init_ssh_pubkey: cloudInitSshKey.trim() || undefined,
        virtio_win_iso: virtioWinIso.trim() || undefined,
      })
      toast.success(`VM '${name}' created — open Console to finish install (same idea as Cockpit Machines).`)
      navigate(vmDetailRoute(name, createLibvirtTarget === 'session' ? 'session' : undefined))
    } catch (e: unknown) {
      setCreateProgressFailed(true)
      toast.error(`Create failed: ${formatUserError(e)}`)
    } finally {
      setSubmitting(false)
    }
  }

  const handleCreateGolden = async () => {
    const name = vmName.trim()
    if (!name) {
      toast.warning('Name is required')
      return
    }
    if (goldenKind === 'template') {
      if (!selectedTemplateName.trim()) {
        toast.warning('Choose a saved template (create one under /var/lib/machina/templates/ or Save template from a VM)')
        return
      }
      const tmpl = savedTemplates.find((x) => x.name === selectedTemplateName.trim())
      if (!tmpl?.base_image?.trim()) {
        toast.warning('That template has no base_image (Packer qcow2 path). Edit the JSON or use “Direct qcow2 backing”.')
        return
      }
    } else {
      if (!backingGoldenPath.trim()) {
        toast.warning('Enter the golden qcow2 path on the hypervisor (or browse)')
        return
      }
      if (goldenOverlayGb < 5) {
        toast.warning('Overlay disk size must be at least 5 GiB')
        return
      }
    }

    const req: CreateVmRequest = {
      name,
      vcpus: goldenKind === 'backing' ? vcpus : 1,
      memory_mb: goldenKind === 'backing' ? memoryMb : 512,
      disk_gb: goldenKind === 'backing' ? goldenOverlayGb : 10,
      network,
      firmware,
      create_backend: 'virt_install',
      graphics_type: graphicsType,
      graphics_listen: graphicsListen.trim() || undefined,
      virt_install_path_in_use_check_off: pathCheckOff || undefined,
    }

    if (goldenKind === 'template') {
      req.saved_template = selectedTemplateName.trim()
      req.template_disk_mode = templateDiskMode
    } else {
      req.virt_install_disk_backing_store = backingGoldenPath.trim()
    }

    if (libSummary?.dual_connection) {
      if (createLibvirtTarget === 'session') req.libvirt_connection = 'session'
      else if (createLibvirtTarget === 'system') req.libvirt_connection = 'system'
    }

    setSubmitting(true)
    setCreateProgressOk(false)
    setCreateProgressFailed(false)
    setCreateLog([])
    try {
      await createVMWithProgress(req, (line) => setCreateLog((prev) => [...prev, line]))
      setCreateProgressOk(true)
      toast.success(`VM '${name}' created from golden image — start it from the VM list.`)
      navigate(vmDetailRoute(name, createLibvirtTarget === 'session' ? 'session' : undefined))
    } catch (e: unknown) {
      setCreateProgressFailed(true)
      toast.error(`Create failed: ${formatUserError(e)}`)
    } finally {
      setSubmitting(false)
    }
  }

  const scrollCreateFormToTop = () => {
    window.scrollTo({ top: 0, behavior: 'smooth' })
  }

  const showSinglePageForm = () => {
    setUseInstallWizard(false)
    scrollCreateFormToTop()
  }

  const showGuidedSteps = () => {
    setUseInstallWizard(true)
    setInstallWizardStep(0)
    scrollCreateFormToTop()
  }

  const cloudInitGenerateBlockedReason = !vmName.trim()
    ? 'Enter a VM name on the Machine details section first.'
    : !cloudInitUser.trim()
      ? 'Enter a cloud-init username.'
      : null

  const sourceTabs: {
    id: InstallSource
    label: string
    hint: string
    icon: LucideIcon
  }[] = [
    { id: 'iso', label: 'Local install media', hint: 'ISO on the hypervisor (like Cockpit “Local install media”)', icon: Disc },
    { id: 'url', label: 'Network install', hint: 'HTTP(S) or NFS tree — virt-install --location', icon: Globe },
    { id: 'pxe', label: 'Network boot (PXE)', hint: 'PXE on a second NIC (Cockpit-style network install)', icon: Network },
    { id: 'download', label: 'Automatic OS install', hint: 'virt-install --install os=… (downloaded media)', icon: Download },
  ]

  return (
    <PageLayout
      eyebrow="Hypervisor"
      className="max-w-6xl mx-auto pb-8"
      contentClassName="space-y-8"
      title="Create new guest VM"
      subtitle={
        <>
          Define a QEMU/KVM guest on this hypervisor host via libvirt—the same <code className="text-[var(--text-muted)]">virt-install</code> style as{' '}
          <span className="text-[var(--text-muted)]">Cockpit Machines</span>: install from media, or clone many identical workers from a Packer golden qcow2. Optional KubeVirt YAML and cluster actions are on the VM&apos;s details page when enabled.
        </>
      }
      icon={<Boxes className="w-6 h-6 text-[var(--accent)]" />}
      actions={
        <Link to="/vms" className="p-2 hover:bg-[var(--surface-hover)] rounded transition" aria-label="Back">
          <ArrowLeft className="w-5 h-5" />
        </Link>
      }
      error={catalogWarning}
      errorTitle="Some catalogs could not be loaded"
      errorHints={catalogWarning ? libvirtErrorHints(catalogWarning) : undefined}
      technicalDetail={catalogWarning}
      errorTone="amber"
      onErrorDismiss={() => setCatalogWarning(null)}
    >
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_17rem] xl:items-start">
      <div className="min-w-0 space-y-8">
      <div>
        <h2 className="text-sm font-semibold text-[var(--text-muted)] uppercase tracking-wide mb-3">How do you want to create this VM?</h2>
        <ChoiceCardGrid>
          <ChoiceCard
            largeIcon
            tone="blue"
            selected={pageFlow === 'install'}
            onClick={() => {
              setPageFlow('install')
              setCreateLog([])
              setCreateProgressOk(false)
              setCreateProgressFailed(false)
            }}
            icon={<Disc className="w-5 h-5" />}
            title="Install from media"
            description="Fresh guest on this host: ISO, URL, PXE, or downloaded OS — same idea as Cockpit Machines."
          />
          <ChoiceCard
            largeIcon
            tone="amber"
            selected={pageFlow === 'golden'}
            onClick={() => {
              setPageFlow('golden')
              setCreateLog([])
              setCreateProgressOk(false)
              setCreateProgressFailed(false)
            }}
            icon={<Layers className="w-5 h-5" />}
            title="Clone from golden image"
            description="Many identical worker guests from a Packer qcow2 — saved template or thin overlay on a golden disk."
          />
          {fluxvmEnabled && (
            <ChoiceCard
              largeIcon
              tone="violet"
              selected={pageFlow === 'fluxvm'}
              onClick={() => setPageFlow('fluxvm')}
              icon={<Zap className="w-5 h-5" />}
              title="FluxVM guest"
              description="QEMU, Cloud Hypervisor or Firecracker via fluxvm-api — lifecycle, metrics and serial console."
            />
          )}
        </ChoiceCardGrid>
      </div>

      {libSummary?.dual_connection && pageFlow !== 'fluxvm' && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)] space-y-3">
          <h3 className="text-sm font-semibold text-[var(--text-primary)]">Hypervisor scope</h3>
          <p className="text-xs text-[var(--text-muted)]">Daemon is merging system + session libvirt (same pattern as Cockpit Machines). Choose where this domain should be defined.</p>
          <div className="flex flex-wrap gap-4 text-sm text-[var(--text-secondary)]">
            <label className="flex items-center gap-2 cursor-pointer">
              <input type="radio" name="machina-lv-scope" checked={createLibvirtTarget === 'default'} onChange={() => setCreateLibvirtTarget('default')} />
              Default
            </label>
            <label className="flex items-center gap-2 cursor-pointer">
              <input type="radio" name="machina-lv-scope" checked={createLibvirtTarget === 'system'} onChange={() => setCreateLibvirtTarget('system')} />
              qemu:///system
            </label>
            <label className="flex items-center gap-2 cursor-pointer">
              <input type="radio" name="machina-lv-scope" checked={createLibvirtTarget === 'session'} onChange={() => setCreateLibvirtTarget('session')} />
              qemu:///session
            </label>
          </div>
        </div>
      )}

      {pageFlow === 'fluxvm' && fluxvmEnabled && (
        <FluxvmCreatePanel
          initialName={vmName}
          defaultHypervisor={platformInfo?.fluxvm?.default_backend}
          onCreated={(created) => {
            toast.success(`FluxVM guest '${created}' created.`)
            navigate(vmDetailRoute(created, 'fluxvm'))
          }}
          onError={(msg) => toast.error(`Create failed: ${msg}`)}
        />
      )}

      {pageFlow === 'install' && (
        <>
      {useInstallWizard && (
        <WizardStepper
          steps={INSTALL_WIZARD_STEPS}
          current={installWizardStep}
          onStep={setInstallWizardStep}
          trailing={
            <button
              type="button"
              data-testid="create-vm-single-page"
              className="text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)] underline"
              onClick={showSinglePageForm}
            >
              Single-page form
            </button>
          }
        />
      )}
      {!useInstallWizard && (
        <div className="text-right mb-2">
          <button
            type="button"
            data-testid="create-vm-guided-steps"
            className="text-xs text-[var(--accent)] hover:underline"
            onClick={showGuidedSteps}
          >
            Use guided steps
          </button>
        </div>
      )}

      {/* Installation source (Cockpit-style) */}
      {(!useInstallWizard || installWizardStep === 0) && (
      <div id="create-vm-step-0" className="scroll-mt-28 space-y-4">
      <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
        <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
          <Disc className={`w-5 h-5 ${statusToneClass('warn')}`} />
          Installation source
        </h2>
        <ChoiceCardGrid>
          {sourceTabs.map((t) => {
            const Icon = t.icon
            return (
              <ChoiceCard
                key={t.id}
                tone="sky"
                selected={installSource === t.id}
                onClick={() => setSource(t.id)}
                icon={<Icon className="w-4 h-4" />}
                title={t.label}
                description={t.hint}
              />
            )
          })}
        </ChoiceCardGrid>

        {installSource === 'iso' && (
          <div className="space-y-3 pt-2 border-t border-[var(--apple-hairline)]">
            {isoScan.length > 0 && (
              <div>
                <label className="block text-sm text-[var(--text-muted)] mb-1">Recently discovered ISOs</label>
                <select
                  aria-label="Recently discovered ISOs"
                  value=""
                  onChange={(e) => e.target.value && setIso(e.target.value)}
                  className="input-field"
                >
                  <option value="">— pick to fill path —</option>
                  {isoScan.map((f) => (
                    <option key={f.path} value={f.path}>
                      {f.name}
                    </option>
                  ))}
                </select>
              </div>
            )}
            <div>
              <label htmlFor="iso-path" className="block text-sm text-[var(--text-muted)] mb-1">
                ISO file on hypervisor *
              </label>
              <div className="flex gap-2">
                <input
                  id="iso-path"
                  type="text"
                  value={iso}
                  onChange={(e) => setIso(e.target.value)}
                  className="input-field flex-1 font-mono text-sm"
                  placeholder="/var/lib/libvirt/images/install.iso"
                />
                <button
                  type="button"
                  onClick={() => {
                    if (canBrowseHost) setIsoBrowseOpen(true)
                  }}
                  disabled={!canBrowseHost}
                  title={!canBrowseHost ? 'Browsing host paths requires the admin role' : undefined}
                  className="px-3 py-2 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-50 rounded-lg text-sm flex items-center gap-2 shrink-0"
                >
                  <FolderOpen className="w-4 h-4" />
                  Browse
                </button>
              </div>
            </div>
          </div>
        )}

        {installSource === 'url' && (
          <div className="space-y-3 pt-2 border-t border-[var(--apple-hairline)]">
            <label htmlFor="loc-url" className="block text-sm text-[var(--text-muted)] mb-1">
              Install URL or directory tree *
            </label>
            <div className="flex flex-wrap gap-2 items-end">
              <input
                id="loc-url"
                type="text"
                value={virtInstallLocation}
                onChange={(e) => setVirtInstallLocation(e.target.value)}
                className="input-field font-mono text-sm flex-1 min-w-[min(100%,16rem)]"
                placeholder="https://download.fedoraproject.org/pub/fedora/linux/releases/44/Server/x86_64/os/"
              />
              <button
                type="button"
                onClick={() => void runOsDetect()}
                disabled={osDetectBusy}
                className="px-3 py-2 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-50 rounded-lg text-sm shrink-0"
              >
                {osDetectBusy ? 'Detecting…' : 'Detect OS'}
              </button>
            </div>
            <p className="text-xs text-[var(--text-muted)]">
              Runs <code className="text-[var(--text-muted)]">osinfo-detect --type=tree</code> on the hypervisor; first stdout token fills OS variant when it succeeds.
            </p>
          </div>
        )}

        {installSource === 'pxe' && (
          <div className="space-y-3 pt-2 border-t border-[var(--apple-hairline)]">
            <p className="text-sm text-[var(--text-muted)]">
              The VM boots with an extra NIC on the libvirt network you choose for PXE (primary NIC stays on “Network” below).
            </p>
            <label htmlFor="pxe-net" className="block text-sm text-[var(--text-muted)] mb-1">
              PXE network (libvirt network name)
            </label>
            <input
              id="pxe-net"
              type="text"
              value={virtInstallPxeNetwork}
              onChange={(e) => setVirtInstallPxeNetwork(e.target.value)}
              className="input-field"
              placeholder="Leave empty to use the same as primary network"
            />
          </div>
        )}

        {installSource === 'download' && (
          <div className="space-y-3 pt-2 border-t border-[var(--apple-hairline)]">
            <label htmlFor="os-preset" className="block text-sm text-[var(--text-muted)] mb-1">
              OS for automatic download *
            </label>
            <p className="text-xs text-[var(--text-muted)] mb-2">
              Pick a profile — values match <code className="text-[var(--text-muted)]">virt-install --install os=…</code> on your libvirt/osinfo-db (no free typing).
            </p>
            <select
              id="os-preset"
              value={virtInstallInstallOs}
              onChange={(e) => {
                const v = e.target.value
                setVirtInstallInstallOs(v)
                const g = MACHINA_PACKER_SCRIPT_GUESTS.find((x) => x.virtInstallDownloadOs === v)
                if (g) setOsVariant(g.osVariantHint)
              }}
              className="input-field max-w-xl"
            >
              {LINUX_PACKER_GUESTS.map((g) => (
                <option key={g.id} value={g.virtInstallDownloadOs}>
                  {g.label} — {g.virtInstallDownloadOs}
                </option>
              ))}
            </select>
            {MACHINA_PACKER_SCRIPT_GUESTS.find((g) => g.virtInstallDownloadOs === virtInstallInstallOs)?.family === 'rpm' && (
              <div className="space-y-2 rounded-lg border border-rose-500/30 bg-rose-950/20 p-3">
                <label htmlFor="rhel-access-token" className="block text-sm text-[var(--text-muted)]">RHEL-family offline image token (POST /guest-images/rhel-url)</label>
                <input
                  id="rhel-access-token"
                  type="password"
                  autoComplete="off"
                  value={rhelAccessToken}
                  onChange={(e) => setRhelAccessToken(e.target.value)}
                  className="input-field font-mono text-xs"
                  placeholder="offline access token"
                />
                <button
                  type="button"
                  data-testid="rhel-image-url"
                  disabled={rhelImageBusy || !rhelAccessToken.trim()}
                  className="px-3 py-2 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-50 rounded-lg text-sm"
                  onClick={() => {
                    setRhelImageBusy(true)
                    void guestRhelImageUrl({ access_token: rhelAccessToken.trim(), rhel_version: '9' })
                      .then((r) => {
                        const raw = r.raw as { href?: string; image?: { href?: string } } | undefined
                        const href = raw?.href ?? raw?.image?.href
                        setRhelImageHint(href ? `Resolved: ${href}` : r.error ?? 'No image URL in response')
                      })
                      .catch((e: unknown) => setRhelImageHint(formatUserError(e)))
                      .finally(() => setRhelImageBusy(false))
                  }}
                >
                  {rhelImageBusy ? 'Resolving…' : 'Resolve RHEL image URL'}
                </button>
                {rhelImageHint && <p className="text-xs text-[var(--text-muted)] break-all" data-testid="rhel-image-result">{rhelImageHint}</p>}
              </div>
            )}
          </div>
        )}

        {(installSource === 'iso' || installSource === 'url' || installSource === 'download') && (
          <div>
            <label htmlFor="extra-args" className="block text-sm text-[var(--text-muted)] mb-1">
              Kernel / installer arguments (optional, virt-install --extra-args)
            </label>
            <textarea
              id="extra-args"
              value={virtInstallExtraArgs}
              onChange={(e) => setVirtInstallExtraArgs(e.target.value)}
              rows={2}
              className="input-field font-mono text-xs"
              placeholder="e.g. inst.ks=http://…/ks.cfg for kickstart"
            />
          </div>
        )}
      </div>

      {libvirtTemplates.length > 0 && (
        <div className="bg-[var(--apple-surface)] rounded-xl p-5 border border-[var(--apple-hairline)] space-y-2" data-testid="libvirt-templates-panel">
          <h2 className="text-base font-semibold text-[var(--text-primary)] flex items-center gap-2">
            <LayoutTemplate className="w-5 h-5 text-[var(--accent)]" aria-hidden />
            Libvirt template catalog
          </h2>
          <p className="text-xs text-[var(--text-muted)]">GET /api/v1/templates — distinct from saved golden templates below.</p>
          <ul className="text-sm text-[var(--text-secondary)] space-y-1">
            {libvirtTemplates.slice(0, 8).map((t) => (
              <li key={t.name} className="font-mono text-[var(--accent)]">
                {t.name}
                <span className="text-[var(--text-muted)] text-xs ml-2">
                  {t.vcpus} vCPU · {t.memory_mb} MiB · {t.disk_gb} GiB
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* VM details */}
      <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
        <h2 className="text-lg font-semibold text-[var(--text-primary)]">Machine details</h2>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div className="sm:col-span-2">
            <label htmlFor="vm-name" className="block text-sm text-[var(--text-muted)] mb-1">
              Name *
            </label>
            <input
              id="vm-name"
              type="text"
              value={vmName}
              onChange={(e) => setVmName(e.target.value)}
              className="input-field"
              placeholder="my-vm"
              autoComplete="off"
            />
          </div>
          <div>
            <label htmlFor="vcpus" className="block text-sm text-[var(--text-muted)] mb-1">
              CPUs
            </label>
            <input
              id="vcpus"
              type="number"
              min={1}
              max={64}
              value={vcpus}
              onChange={(e) => setVcpus(Number(e.target.value) || 1)}
              className="input-field"
            />
          </div>
          <div>
            <label htmlFor="mem" className="block text-sm text-[var(--text-muted)] mb-1">
              Memory (MiB)
            </label>
            <input
              id="mem"
              type="number"
              min={256}
              step={256}
              value={memoryMb}
              onChange={(e) => setMemoryMb(Number(e.target.value) || 1024)}
              className="input-field"
            />
          </div>
          <div>
            <label htmlFor="fw" className="block text-sm text-[var(--text-muted)] mb-1">
              Firmware
            </label>
            <select id="fw" value={firmware} onChange={(e) => setFirmware(e.target.value)} className="input-field">
              <option value="bios">BIOS</option>
              <option value="uefi">UEFI</option>
            </select>
          </div>
          <div className="space-y-1">
            <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
              <input
                type="checkbox"
                checked={secureBoot && firmware === 'uefi'}
                disabled={firmware !== 'uefi'}
                onChange={(e) => setSecureBoot(e.target.checked)}
              />{' '}
              Secure Boot (keys enrolled)
            </label>
            <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
              <input type="checkbox" checked={vtpm} onChange={(e) => setVtpm(e.target.checked)} /> TPM 2.0 (emulated)
            </label>
            <p className="text-xs text-[var(--text-muted)]">
              Windows 11 needs both. Secure Boot requires UEFI; the host needs signed OVMF firmware and swtpm.
            </p>
          </div>
          <div>
            <label htmlFor="guest-profile" className="block text-sm text-[var(--text-muted)] mb-1">
              Guest profile
            </label>
            <select
              id="guest-profile"
              value={guestProfile}
              onChange={(e) => setGuestProfile(e.target.value as GuestProfile)}
              className="input-field"
            >
              <option value="auto">Auto</option>
              <option value="linux">Linux</option>
              <option value="windows">Windows (SPICE + virtio)</option>
            </select>
            <p className="text-xs text-[var(--text-muted)] mt-1">
              Windows profile enables SPICE/QXL and virtio disk/NIC defaults on the server. Attach a virtio-win ISO below if your Windows installer needs drivers.
            </p>
          </div>
          {installSource === 'download' ? (
            <div>
              <label className="block text-sm text-[var(--text-muted)] mb-1">
                <code className="text-[var(--text-secondary)]">--os-variant</code> (from your OS pick)
              </label>
              <p className="text-sm font-mono text-[var(--text-primary)] bg-[var(--apple-surface)] border border-[var(--apple-hairline)] rounded-lg px-3 py-2">{osVariant || 'generic'}</p>
            </div>
          ) : (
            <div>
              <label htmlFor="osv" className="block text-sm text-[var(--text-muted)] mb-1">
                Operating system (optional)
              </label>
              <datalist id="machina-os-variant-list">
                {guestOsRows.map((o) => (
                  <option key={o.short_id} value={o.short_id}>
                    {o.name} {o.version}
                  </option>
                ))}
              </datalist>
              <input
                id="osv"
                type="text"
                value={osVariant}
                onChange={(e) => setOsVariant(e.target.value)}
                className="input-field"
                placeholder="libosinfo id — empty = generic"
                list="machina-os-variant-list"
              />
              <p className="text-xs text-[var(--text-muted)] mt-1">
                Pick from libvirt/osinfo suggestions or type a short id (see also Detect OS for URL installs).
              </p>
            </div>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2 pt-4 mt-2 border-t border-[var(--apple-hairline)]">
          <button
            type="button"
            className="text-xs px-3 py-1.5 rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)]/80 hover:bg-[var(--surface-hover)] text-[var(--text-primary)] transition"
            onClick={saveBrowserDefaults}
          >
            Save defaults (browser)
          </button>
          {(sessionRole === 'admin' || sessionRole === 'operator') && (
            <button
              type="button"
              className="text-xs px-3 py-1.5 rounded-lg border border-[var(--accent)]/40 bg-[var(--accent-soft)] hover:bg-[var(--accent-soft)] text-[var(--accent)] transition"
              onClick={() => void saveServerDefaults()}
            >
              Save defaults (hypervisor)
            </button>
          )}
          <span className="text-xs text-[var(--text-muted)]">On load: server defaults, then browser overrides.</span>
        </div>
      </div>
      </div>
      )}

      {/* Storage — Cockpit: new image vs existing volume */}
      {(!useInstallWizard || installWizardStep === 1) && (
      <div id="create-vm-step-1" className="scroll-mt-28">
      <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
        <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
          <HardDrive className="w-5 h-5 text-[var(--link)]" />
          Storage
        </h2>
        <ChoiceCardGrid>
          <ChoiceCard
            tone="cyan"
            selected={storageMode === 'new'}
            onClick={() => setStorageMode('new')}
            icon={<HardDrive className="w-4 h-4" />}
            title="Create new disk image"
            description="Let virt-install allocate a new qcow2 in the default pool (set size below)."
          />
          <ChoiceCard
            tone="cyan"
            selected={storageMode === 'volume'}
            onClick={() => setStorageMode('volume')}
            icon={<Boxes className="w-4 h-4" />}
            title="Use existing storage volume"
            description="Attach an empty volume from a pool (e.g. with ISO + existing vol for Cockpit-style installs)."
          />
        </ChoiceCardGrid>
        {storageMode === 'new' ? (
          <div>
            <label htmlFor="disk-gb" className="block text-sm text-[var(--text-muted)] mb-1">
              Disk size (GiB)
            </label>
            <input
              id="disk-gb"
              type="number"
              min={5}
              value={diskGb}
              onChange={(e) => setDiskGb(Number(e.target.value) || 10)}
              className="input-field max-w-xs"
            />
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div>
              <label htmlFor="pool" className="block text-sm text-[var(--text-muted)] mb-1">
                Storage pool *
              </label>
              <select
                id="pool"
                value={diskPool}
                onChange={(e) => {
                  setDiskPool(e.target.value)
                  setDiskVol('')
                }}
                className="input-field"
              >
                {pools.length === 0 ? <option value="">No pools (refresh libvirt)</option> : null}
                {pools.map((p) => (
                  <option key={p.name} value={p.name}>
                    {p.name} ({p.state})
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label htmlFor="vol" className="block text-sm text-[var(--text-muted)] mb-1">
                Volume *
              </label>
              <select
                id="vol"
                value={diskVol}
                onChange={(e) => setDiskVol(e.target.value)}
                className="input-field"
                disabled={!diskPool || volumes.length === 0}
              >
                <option value="">{volumes.length ? '— select volume —' : '— no volumes —'}</option>
                {volumes.map((v) => (
                  <option key={v.name} value={v.name}>
                    {v.name} ({v.capacity_gb} GiB)
                  </option>
                ))}
              </select>
            </div>
          </div>
        )}
      </div>
      </div>
      )}

      {/* Network + console */}
      {(!useInstallWizard || installWizardStep === 2) && (
      <div id="create-vm-step-2" className="scroll-mt-28">
      <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
        <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
          <Network className={`w-5 h-5 ${statusToneClass('ok')}`} />
          Networking
        </h2>
        <div>
          <label htmlFor="net" className="block text-sm text-[var(--text-muted)] mb-1">
            Virtual network (NAT / bridge)
          </label>
          <select id="net" value={network} onChange={(e) => setNetwork(e.target.value)} className="input-field max-w-md">
            {networks.length === 0 ? <option value="default">default</option> : null}
            {networks.map((n) => (
              <option key={n.name} value={n.name}>
                {n.name}
              </option>
            ))}
          </select>
        </div>

        <h3 className="text-sm font-semibold text-[var(--text-primary)] flex items-center gap-2 pt-2 border-t border-[var(--apple-hairline)]">
          <Monitor className="w-4 h-4 text-[var(--accent)]" />
          Console (remote viewer)
        </h3>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <label htmlFor="gfx-type" className="block text-sm text-[var(--text-muted)] mb-1">
              Graphics
            </label>
            <select id="gfx-type" value={graphicsType} onChange={(e) => setGraphicsType(e.target.value as 'vnc' | 'spice')} className="input-field">
              <option value="vnc">VNC</option>
              <option value="spice">SPICE</option>
            </select>
          </div>
          <div>
            <label htmlFor="gfx-listen" className="block text-sm text-[var(--text-muted)] mb-1">
              Listen address
            </label>
            <select id="gfx-listen" value={graphicsListen} onChange={(e) => setGraphicsListen(e.target.value)} className="input-field">
              <option value="127.0.0.1">Localhost only (default, secure)</option>
              <option value="0.0.0.0">All interfaces (remote viewer like Cockpit)</option>
            </select>
          </div>
        </div>

        {guestProfile === 'windows' && (
          <div className="pt-2 border-t border-[var(--apple-hairline)] space-y-2">
            <label htmlFor="virtio-win-iso" className="block text-sm text-[var(--text-muted)] mb-1">virtio-win drivers ISO (recommended for Windows installs)</label>
            <div className="flex gap-2">
              <input
                id="virtio-win-iso"
                type="text"
                value={virtioWinIso}
                onChange={(e) => setVirtioWinIso(e.target.value)}
                className="input-field flex-1 font-mono text-sm"
                placeholder="/var/lib/libvirt/images/virtio-win.iso"
              />
              <button
                type="button"
                onClick={() => {
                  if (canBrowseHost) setVirtioBrowseOpen(true)
                }}
                disabled={!canBrowseHost}
                title={!canBrowseHost ? 'Browsing host paths requires the admin role' : undefined}
                className="px-3 py-2 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-50 rounded-lg text-sm flex items-center gap-2 shrink-0"
              >
                <FolderOpen className="w-4 h-4" />
                Browse
              </button>
            </div>
            <p className="text-xs text-[var(--text-muted)]">
              This is attached as an extra CD-ROM so Windows Setup can load virtio storage/network drivers (Win10/11/Server 2019/2022).
            </p>
          </div>
        )}
      </div>
      </div>
      )}

      {/* Optional cloud-init CD */}
      {(!useInstallWizard || installWizardStep === 3) && (
      <div id="create-vm-step-3" className="scroll-mt-28">
      <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-3">
        <h2 className="text-base font-semibold text-[var(--text-primary)]">Cloud-init / seed ISO (optional)</h2>
        <p className="text-xs text-[var(--text-muted)]">
          Second CD-ROM for NoCloud/autoinstall. You can either attach an existing seed ISO, or let machina generate one (Cockpit-style).
        </p>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <div>
            <label htmlFor="cloud-init-user" className="block text-sm text-[var(--text-muted)] mb-1">User</label>
            <input id="cloud-init-user" value={cloudInitUser} onChange={(e) => setCloudInitUser(e.target.value)} className="input-field" placeholder="ubuntu" />
          </div>
          <div>
            <label htmlFor="cloud-init-password" className="block text-sm text-[var(--text-muted)] mb-1">Password (optional)</label>
            <input id="cloud-init-password" value={cloudInitPassword} onChange={(e) => setCloudInitPassword(e.target.value)} className="input-field" placeholder="(leave blank to skip)" />
          </div>
          <div>
            <label htmlFor="cloud-init-ssh-key" className="block text-sm text-[var(--text-muted)] mb-1">SSH public key (optional)</label>
            <input id="cloud-init-ssh-key" value={cloudInitSshKey} onChange={(e) => setCloudInitSshKey(e.target.value)} className="input-field font-mono text-xs" placeholder="ssh-ed25519 AAAA..." />
          </div>
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            aria-label="Cloud-init ISO path on hypervisor"
            value={cloudInitIso}
            onChange={(e) => setCloudInitIso(e.target.value)}
            className="input-field flex-1 font-mono text-sm"
            placeholder="Absolute path on hypervisor"
          />
          <button
            type="button"
            onClick={() => {
              if (canBrowseHost) setCloudBrowseOpen(true)
            }}
            disabled={!canBrowseHost}
            title={!canBrowseHost ? 'Browsing host paths requires the admin role' : undefined}
            className="px-3 py-2 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-50 rounded-lg text-sm flex items-center gap-2 shrink-0"
          >
            <FolderOpen className="w-4 h-4" />
            Browse
          </button>
        </div>
        <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)] cursor-pointer">
          <input type="checkbox" checked={pathCheckOff} onChange={(e) => setPathCheckOff(e.target.checked)} className="rounded" />
          Ignore path-in-use check (busy images / volumes)
        </label>
        {cloudInitGenerateBlockedReason && (
          <p className="text-xs text-amber-400/90" data-testid="cloud-init-generate-hint">
            {cloudInitGenerateBlockedReason}
          </p>
        )}
        <button
          type="button"
          data-testid="cloud-init-generate"
          disabled={cloudInitGenBusy || !vmName.trim() || !cloudInitUser.trim()}
          className="btn-primary text-sm disabled:opacity-50"
          onClick={() => {
            setCloudInitGenBusy(true)
            void generateCloudInit(
              vmName.trim(),
              cloudInitUser.trim(),
              cloudInitPassword,
              cloudInitSshKey,
            )
              .then((r) => {
                setCloudInitIso(r.path)
                toast.success(`Seed ISO: ${r.path}`)
              })
              .catch((e: unknown) => toast.error(formatUserError(e)))
              .finally(() => setCloudInitGenBusy(false))
          }}
        >
          {cloudInitGenBusy ? 'Generating…' : 'Generate seed ISO (POST /cloud-init)'}
        </button>
      </div>
      </div>
      )}

      {useInstallWizard && (
        <div className="flex flex-wrap justify-between items-center gap-2 bg-[var(--apple-fill-tertiary)]/30 rounded-lg px-3 py-2 border border-[var(--apple-hairline)]/40">
          <button
            type="button"
            disabled={installWizardStep === 0}
            onClick={() => setInstallWizardStep((s) => Math.max(0, s - 1))}
            className="px-4 py-2 rounded-lg bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-40 text-sm"
          >
            Back
          </button>
          <span className="text-sm text-[var(--text-muted)]">
            Step {installWizardStep + 1} of {INSTALL_WIZARD_STEPS.length}
          </span>
          {installWizardStep < 4 ? (
            <button
              type="button"
              onClick={() => setInstallWizardStep((s) => Math.min(4, s + 1))}
              className="btn-primary text-sm"
            >
              Next
            </button>
          ) : (
            <span className="w-20 sm:w-24" aria-hidden />
          )}
        </div>
      )}

      {useInstallWizard && installWizardStep === 4 && (
        <div id="create-vm-step-4" className="scroll-mt-28 bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--accent)]/40 space-y-2 text-sm text-[var(--text-secondary)]">
          <h2 className="text-lg font-semibold text-[var(--text-primary)]">Review</h2>
          <p>
            <span className="text-[var(--text-muted)]">Name:</span> {vmName.trim() || '—'}
          </p>
          <p>
            <span className="text-[var(--text-muted)]">Install source:</span> {installSource}
          </p>
          <p>
            <span className="text-[var(--text-muted)]">CPU / RAM / disk:</span> {vcpus} vCPU — {memoryMb} MiB —{' '}
            {storageMode === 'new' ? (
              <span>new {diskGb} GiB</span>
            ) : (
              <span>
                pool <code className="text-[var(--text-secondary)]">{diskPool || '—'}</code> / vol{' '}
                <code className="text-[var(--text-secondary)]">{diskVol || '—'}</code>
              </span>
            )}
          </p>
          <p>
            <span className="text-[var(--text-muted)]">Network / console:</span> {network} — {graphicsType} @ {graphicsListen}
          </p>
          <p>
            <span className="text-[var(--text-muted)]">Cloud-init user:</span> {cloudInitUser.trim() || '—'}
          </p>
          <p className="text-xs text-[var(--text-muted)] pt-1">
            Sane defaults for this browser + hypervisor (
            <code className="text-[var(--text-muted)]">{libSummary?.configured_uri ?? 'default'}</code>
            ) are saved to local storage after a successful create.
          </p>
        </div>
      )}

      {(!useInstallWizard || installWizardStep === 4) && (
        <button
          type="button"
          onClick={handleCreate}
          disabled={submitting}
          className="btn-primary text-sm disabled:opacity-50 w-full sm:w-auto px-8 py-3"
        >
          {submitting ? 'Creating…' : 'Create VM (install continues in Console)'}
        </button>
      )}
        </>
      )}

      {pageFlow === 'golden' && (
        <div className="space-y-6">
          <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-amber-900/40 space-y-4">
            <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
              <Layers className={`w-5 h-5 ${statusToneClass('warn')}`} />
              Golden image source
            </h2>
            <p className="text-sm text-[var(--text-muted)]">
              After Packer writes e.g. <code className="text-[var(--text-secondary)]">output-fedora43/fedora43.qcow2</code>, keep one canonical file on the host and reuse it: either register a{' '}
              <span className="text-[var(--text-primary)]">saved template</span> JSON under{' '}
              <code className="text-[var(--text-secondary)]">/var/lib/machina/templates/</code> with <code className="text-[var(--text-secondary)]">base_image</code> pointing at that path, or attach the qcow2 directly as a{' '}
              <span className="text-[var(--text-primary)]">backing store</span> (thin overlay per VM).
            </p>
            <ChoiceCardGrid>
              <ChoiceCard
                tone="amber"
                selected={goldenKind === 'template'}
                onClick={() => setGoldenKind('template')}
                icon={<LayoutTemplate className="w-4 h-4" />}
                title="Saved template"
                description={
                  <>
                    JSON under <code className="text-[var(--text-muted)]">/var/lib/machina/templates/</code> with{' '}
                    <code className="text-[var(--text-muted)]">base_image</code>.
                  </>
                }
              />
              <ChoiceCard
                tone="amber"
                selected={goldenKind === 'backing'}
                onClick={() => setGoldenKind('backing')}
                icon={<Link2 className="w-4 h-4" />}
                title="Direct qcow2 backing"
                description="Point at one golden qcow2; each VM gets a thin overlay disk on top."
              />
            </ChoiceCardGrid>

            {goldenKind === 'template' && (
              <div className="space-y-3 pt-2 border-t border-[var(--apple-hairline)]">
                <label htmlFor="tmpl-sel" className="block text-sm text-[var(--text-muted)] mb-1">
                  Template *
                </label>
                <select
                  id="tmpl-sel"
                  value={selectedTemplateName}
                  onChange={(e) => setSelectedTemplateName(e.target.value)}
                  className="input-field"
                >
                  <option value="">— select —</option>
                  {savedTemplates.map((t) => (
                    <option key={t.name} value={t.name}>
                      {t.name}
                      {t.base_image ? ' (has golden qcow2)' : ' (no base_image)'}
                    </option>
                  ))}
                </select>
                {savedTemplates.length === 0 && (
                  <p className={`text-xs ${statusToneClass('warn')} opacity-90`}>
                    No templates found. Add <code className="text-[var(--text-secondary)]">/var/lib/machina/templates/mytmpl.json</code> with{' '}
                    <code className="text-[var(--text-secondary)]">base_image</code> set to your Packer qcow2 path, or use <span className="text-[var(--text-primary)]">Save template</span> on a VM details page.
                  </p>
                )}
                <div>
                  <label htmlFor="tmpl-mode" className="block text-sm text-[var(--text-muted)] mb-1">
                    New disk from golden
                  </label>
                  <select
                    id="tmpl-mode"
                    value={templateDiskMode}
                    onChange={(e) => setTemplateDiskMode(e.target.value as 'backing' | 'copy')}
                    className="input-field max-w-md"
                  >
                    <option value="backing">Thin clone (qemu-img backing → small overlay)</option>
                    <option value="copy">Full copy (standalone qcow2)</option>
                  </select>
                </div>
                {selectedTemplateName && (
                  <p className="text-xs text-[var(--text-muted)]">
                    {(() => {
                      const t = savedTemplates.find((x) => x.name === selectedTemplateName)
                      if (!t) return null
                      return (
                        <>
                          Template sizing: {t.vcpus} vCPU, {t.memory_mb} MiB RAM, disk hint {t.disk_gb} GiB, os_variant{' '}
                          <code className="text-[var(--text-muted)]">{t.os_variant || 'generic'}</code>
                          {t.base_image ? (
                            <>
                              . Golden: <code className="text-[var(--text-muted)] break-all">{t.base_image}</code>
                            </>
                          ) : (
                            <span className={`${statusToneClass('warn')} opacity-90`}> — add base_image in JSON for Packer golden reuse.</span>
                          )}
                        </>
                      )
                    })()}
                  </p>
                )}
              </div>
            )}

            {goldenKind === 'backing' && (
              <div className="space-y-3 pt-2 border-t border-[var(--apple-hairline)]">
                <label htmlFor="golden-path" className="block text-sm text-[var(--text-muted)] mb-1">
                  Golden qcow2 on hypervisor *
                </label>
                <div className="flex gap-2">
                  <input
                    id="golden-path"
                    type="text"
                    value={backingGoldenPath}
                    onChange={(e) => setBackingGoldenPath(e.target.value)}
                    className="input-field flex-1 font-mono text-sm"
                    placeholder="/var/lib/libvirt/images/fedora43.qcow2"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      if (canBrowseHost) setBackingBrowseOpen(true)
                    }}
                    disabled={!canBrowseHost}
                    title={!canBrowseHost ? 'Browsing host paths requires the admin role' : undefined}
                    className="px-3 py-2 bg-[var(--surface-hover)] hover:bg-[var(--surface-hover)] disabled:opacity-50 rounded-lg text-sm flex items-center gap-2 shrink-0"
                  >
                    <FolderOpen className="w-4 h-4" />
                    Browse
                  </button>
                </div>
                <div>
                  <label htmlFor="golden-gb" className="block text-sm text-[var(--text-muted)] mb-1">
                    Overlay disk size (GiB)
                  </label>
                  <input
                    id="golden-gb"
                    type="number"
                    min={5}
                    value={goldenOverlayGb}
                    onChange={(e) => setGoldenOverlayGb(Number(e.target.value) || 20)}
                    className="input-field max-w-xs"
                  />
                </div>
              </div>
            )}
          </div>

          <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
            <h2 className="text-lg font-semibold text-[var(--text-primary)]">New VM</h2>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div className="sm:col-span-2">
                <label htmlFor="g-vm-name" className="block text-sm text-[var(--text-muted)] mb-1">
                  Name *
                </label>
                <input
                  id="g-vm-name"
                  type="text"
                  value={vmName}
                  onChange={(e) => setVmName(e.target.value)}
                  className="input-field"
                  placeholder="clone-01"
                />
              </div>
              {goldenKind === 'backing' && (
                <>
                  <div>
                    <label htmlFor="g-vcpus" className="block text-sm text-[var(--text-muted)] mb-1">
                      vCPUs
                    </label>
                    <input
                      id="g-vcpus"
                      type="number"
                      min={1}
                      value={vcpus}
                      onChange={(e) => setVcpus(Number(e.target.value) || 1)}
                      className="input-field"
                    />
                  </div>
                  <div>
                    <label htmlFor="g-mem" className="block text-sm text-[var(--text-muted)] mb-1">
                      Memory (MiB)
                    </label>
                    <input
                      id="g-mem"
                      type="number"
                      min={256}
                      step={256}
                      value={memoryMb}
                      onChange={(e) => setMemoryMb(Number(e.target.value) || 1024)}
                      className="input-field"
                    />
                  </div>
                </>
              )}
              {goldenKind === 'template' && (
                <p className="sm:col-span-2 text-sm text-[var(--text-muted)]">
                  vCPU, RAM, and OS variant for this clone come from the template JSON (applied on the server).
                </p>
              )}
              <div>
                <label htmlFor="g-fw" className="block text-sm text-[var(--text-muted)] mb-1">
                  Firmware
                </label>
                <select id="g-fw" value={firmware} onChange={(e) => setFirmware(e.target.value)} className="input-field">
                  <option value="bios">BIOS</option>
                  <option value="uefi">UEFI</option>
                </select>
              </div>
            </div>
          </div>

          <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-[var(--apple-hairline)] space-y-4">
            <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
              <Network className={`w-5 h-5 ${statusToneClass('ok')}`} />
              Networking &amp; console
            </h2>
            <div>
              <label htmlFor="g-net" className="block text-sm text-[var(--text-muted)] mb-1">
                Virtual network
              </label>
              <select id="g-net" value={network} onChange={(e) => setNetwork(e.target.value)} className="input-field max-w-md">
                {networks.length === 0 ? <option value="default">default</option> : null}
                {networks.map((n) => (
                  <option key={n.name} value={n.name}>
                    {n.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div>
                <label htmlFor="g-gfx" className="block text-sm text-[var(--text-muted)] mb-1">
                  Graphics
                </label>
                <select id="g-gfx" value={graphicsType} onChange={(e) => setGraphicsType(e.target.value as 'vnc' | 'spice')} className="input-field">
                  <option value="vnc">VNC</option>
                  <option value="spice">SPICE</option>
                </select>
              </div>
              <div>
                <label htmlFor="g-listen" className="block text-sm text-[var(--text-muted)] mb-1">
                  Listen
                </label>
                <select id="g-listen" value={graphicsListen} onChange={(e) => setGraphicsListen(e.target.value)} className="input-field">
                  <option value="127.0.0.1">127.0.0.1</option>
                  <option value="0.0.0.0">0.0.0.0</option>
                </select>
              </div>
            </div>
            <label className="flex items-center gap-2 text-sm text-[var(--text-secondary)] cursor-pointer">
              <input type="checkbox" checked={pathCheckOff} onChange={(e) => setPathCheckOff(e.target.checked)} className="rounded" />
              Ignore path-in-use check
            </label>
          </div>

          <button
            type="button"
            onClick={handleCreateGolden}
            disabled={submitting}
            className="w-full sm:w-auto px-8 py-3 bg-amber-600 hover:bg-amber-700 disabled:opacity-50 rounded-lg text-sm font-medium transition"
          >
            {submitting ? 'Creating…' : 'Create VM from golden image'}
          </button>
        </div>
      )}

      {(submitting || createLog.length > 0) && (
        <div className="rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-3 space-y-2">
          <h4 className="text-xs font-semibold text-[var(--text-secondary)]">Guest install (virt-install / mkosi)</h4>
          <BuildStepTimeline
            steps={VM_CREATE_TIMELINE_LABELS}
            activeIndex={vmCreateTimeline.activeIndex}
            allComplete={vmCreateTimeline.allComplete}
            failed={vmCreateTimeline.failed}
            variant="amber"
          />
          <pre className="max-h-56 overflow-y-auto rounded bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] p-2 text-[11px] font-mono text-[var(--text-primary)] whitespace-pre-wrap break-all">
            {createLog.length ? createLog.join('\n') : <span className="text-[var(--text-muted)]">Starting…</span>}
          </pre>
          <div ref={logEndRef} />
        </div>
      )}

      {/* Golden Forge — local Packer qcow2 (job + live logs, same pattern as disk builds) */}
      <div className="bg-[var(--apple-surface)] rounded-xl p-6 border border-violet-800/40 space-y-4">
        <div className="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <h2 className="text-lg font-semibold text-[var(--text-primary)] flex items-center gap-2">
              <Terminal className="w-5 h-5 text-[var(--accent)]" aria-hidden />
              Golden Forge
            </h2>
            <p className="text-sm text-[var(--text-muted)] max-w-3xl mt-1">
              Build a reusable qcow2 on this host with the bundled script, stream logs here and under{' '}
              <Link to="/jobs" className="text-[var(--link)] hover:underline">
                Jobs
              </Link>
              . Default Linux login is usually <code className="text-[var(--text-secondary)]">packer</code> or <code className="text-[var(--text-secondary)]">root</code> with password <code className="text-[var(--text-secondary)]">password</code>. Windows profiles use dockur (<code className="text-[var(--text-secondary)]">Docker</code> / <code className="text-[var(--text-secondary)]">admin</code>) — rotate before production.
            </p>
          </div>
        </div>
        <p className="text-xs text-[var(--text-muted)]">
          Linux: <code className="text-[var(--text-muted)]">{PACKER_SCRIPT_SYSTEM}</code> · Windows:{' '}
          <code className="text-[var(--text-muted)]">{PACKER_WINDOWS_DOCKUR_SYSTEM}</code> · repo:{' '}
          <code className="text-[var(--text-muted)]">{PACKER_SCRIPT_REPO}</code>,{' '}
          <code className="text-[var(--text-muted)]">{PACKER_WINDOWS_DOCKUR_REPO}</code>
        </p>
        <div className="flex flex-wrap items-end gap-3">
          <div className="min-w-[12rem]">
            <label htmlFor="packer-guest" className="block text-sm text-[var(--text-muted)] mb-1">
              Profile
            </label>
            <select
              id="packer-guest"
              value={packerGuestId}
              onChange={(e) => setPackerGuestId(e.target.value)}
              className="input-field w-full max-w-md"
              disabled={packerRunning}
            >
              <optgroup label="Linux (Packer / QEMU)">
                {LINUX_PACKER_GUESTS.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.label} ({g.id})
                  </option>
                ))}
              </optgroup>
              <optgroup label="Windows (dockur / Podman)">
                {WINDOWS_PACKER_GUESTS.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.label} ({g.id})
                  </option>
                ))}
              </optgroup>
            </select>
          </div>
          <button
            type="button"
            onClick={() => void runPackerGoldenBuild()}
            disabled={packerRunning}
            className="btn-primary text-sm disabled:opacity-50 inline-flex items-center gap-2"
          >
            <Terminal className="w-4 h-4" />
            {packerRunning ? 'Building…' : 'Build golden qcow2'}
          </button>
          <Link
            to="/jobs"
            className="inline-flex items-center px-4 py-2.5 rounded-lg text-sm border border-[var(--apple-hairline)] text-[var(--text-primary)] hover:bg-[var(--apple-fill-tertiary)]"
          >
            Open Jobs
          </Link>
        </div>
        {(packerRunning || packerLog.length > 0) && (
          <div className="rounded-lg border border-[var(--apple-hairline)] bg-[var(--apple-fill-tertiary)] p-3 space-y-2">
            <h4 className="text-xs font-semibold text-[var(--link)]">Golden Forge build output</h4>
            <BuildStepTimeline
              steps={GOLDEN_FORGE_TIMELINE_LABELS}
              activeIndex={goldenForgeTimeline.activeIndex}
              allComplete={goldenForgeTimeline.allComplete}
              failed={goldenForgeTimeline.failed}
              variant="violet"
            />
            <pre className="max-h-72 overflow-y-auto rounded bg-[var(--apple-fill-tertiary)] border border-[var(--apple-hairline)] p-2 text-[11px] font-mono text-[var(--text-primary)] whitespace-pre-wrap break-all">
              {packerLog.length ? packerLog.join('\n') : <span className="text-[var(--text-muted)]">Starting…</span>}
            </pre>
            <div ref={packerLogEndRef} />
          </div>
        )}
        <p className="text-sm text-[var(--text-muted)]">
          Then use <span className="text-[var(--text-primary)]">Clone from golden image</span> above, <Link to="/import" className="text-[var(--link)] hover:underline">Import disk</Link>, or a saved template with <code className="text-[var(--text-secondary)]">base_image</code>.
        </p>
      </div>

      <div className="flex flex-wrap gap-3">
        <Link to="/import" className="btn-primary text-sm">
          Import disk
        </Link>
        <Link to="/disk-images" className="inline-flex items-center justify-center btn-secondary text-sm transition">
          Disk images
        </Link>
      </div>

      <BrowseHostPathModal
        open={isoBrowseOpen}
        onClose={() => setIsoBrowseOpen(false)}
        title="Browse for install ISO"
        canSelectFile={isIsoFileName}
        onSelectPath={(p) => {
          setIso(p)
          setIsoBrowseOpen(false)
        }}
      />
      <BrowseHostPathModal
        open={cloudBrowseOpen}
        onClose={() => setCloudBrowseOpen(false)}
        title="Browse for cloud-init / seed ISO"
        canSelectFile={isIsoFileName}
        onSelectPath={(p) => {
          setCloudInitIso(p)
          setCloudBrowseOpen(false)
        }}
      />
      <BrowseHostPathModal
        open={virtioBrowseOpen}
        onClose={() => setVirtioBrowseOpen(false)}
        title="Browse for virtio-win ISO"
        canSelectFile={isIsoFileName}
        onSelectPath={(p) => {
          setVirtioWinIso(p)
          setVirtioBrowseOpen(false)
        }}
      />
      <BrowseHostPathModal
        open={backingBrowseOpen}
        onClose={() => setBackingBrowseOpen(false)}
        title="Browse for golden qcow2"
        canSelectFile={isHostDiskImageFileName}
        onSelectPath={(p) => {
          setBackingGoldenPath(p)
          setBackingBrowseOpen(false)
        }}
      />
      </div>
      <aside className={pageFlow === 'fluxvm' ? 'hidden' : 'hidden xl:block sticky top-20'} aria-label="Summary" data-testid="create-vm-summary">
        <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)] p-4">
          <p className="text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--text-muted)]">Summary</p>
          <p className="mt-1 truncate text-base font-semibold text-[var(--text-primary)]">{vmName.trim() || 'Unnamed guest'}</p>
          <p className="text-xs text-[var(--text-muted)]">{pageFlow === 'golden' ? 'Clone from golden image' : 'Install from media'}</p>
          <dl className="mt-3 divide-y divide-[var(--apple-hairline)] text-sm">
            {[
              ['vCPUs', String(vcpus)],
              ['Memory', formatSummaryMemory(Number(memoryMb))],
              ['Disk', `${pageFlow === 'golden' ? goldenOverlayGb ?? '—' : diskGb ?? '—'} GiB`],
              ['Network', network || 'default'],
              ['Firmware', String(firmware ?? '').toUpperCase() || '—'],
              ['Graphics', String(graphicsType ?? '').toUpperCase() || '—'],
              ...(osVariant ? [['OS variant', osVariant]] : []),
            ].map(([k, v]) => (
              <div key={k} className="flex items-center justify-between gap-3 py-2">
                <dt className="text-[var(--text-muted)]">{k}</dt>
                <dd className="min-w-0 truncate text-right font-medium text-[var(--text-primary)] tabular-nums">{v}</dd>
              </div>
            ))}
          </dl>
        </div>
      </aside>
      </div>
    </PageLayout>
  )
}
