// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { Globe, Network } from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import PlatformStepWizard from '../../components/platform/PlatformStepWizard'
import VmWizardSizeStep, { sizeStepValid, type VmWizardSizeState } from '../../components/platform/VmWizardSizeStep'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import {
  createVmFromVirtInstall,
  listPlatformHosts,
  listPlatformNetworks,
} from '../../api/platform'
import { listStoragePools } from '../../api/platformStorage'
import { queryHostLibvirt } from '../../api/platformVmLibvirt'
import { parseOsinfoDetectVariant } from '../../utils/osinfoDetect'
import { sizeToSpec } from '../../components/platform/vmWizardCatalog'
import { MACHINA_PACKER_SCRIPT_GUESTS } from '../../data/packerGuests'
import { useToastContext } from '../../contexts/ToastContext'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { formatUserError } from '../../utils/apiError'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import { guestRhelImageUrl } from '../../api/guestImages'

type InstallSource = 'url' | 'pxe' | 'download' | 'define' | 'import'
type StorageMode = 'new' | 'existing' | 'volume' | 'backing'

const STEPS = ['Install source', 'Name & size', 'Network', 'Review']

export default function PlatformVirtInstallCreate() {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [step, setStep] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [installSource, setInstallSource] = useState<InstallSource>('download')
  const [locationUrl, setLocationUrl] = useState('')
  const [installOs, setInstallOs] = useState('ubuntu2404')
  const [extraArgs, setExtraArgs] = useState('')
  const [osVariant, setOsVariant] = useState('generic')
  const [firmware, setFirmware] = useState<'bios' | 'uefi'>('uefi')
  const [vmName, setVmName] = useState('vm-install')
  const [sizeState, setSizeState] = useState<VmWizardSizeState>({
    size: 'medium',
    customCores: 4,
    customMemoryGiB: 8,
    customDiskGiB: 40,
  })
  const [network, setNetwork] = useState('default')
  const [pxeNetwork, setPxeNetwork] = useState('default')
  const [networks, setNetworks] = useState<{ name: string; bridge?: string | null }[]>([])
  const [storageMode, setStorageMode] = useState<StorageMode>('new')
  const [existingDisk, setExistingDisk] = useState('')
  const [diskPool, setDiskPool] = useState('')
  const [diskVol, setDiskVol] = useState('')
  const [backingStore, setBackingStore] = useState('')
  const [installIso, setInstallIso] = useState('')
  const [pathCheckOff, setPathCheckOff] = useState(false)
  const [cloudInitUser, setCloudInitUser] = useState('ubuntu')
  const [cloudInitPassword, setCloudInitPassword] = useState('')
  const [cloudInitSshKey, setCloudInitSshKey] = useState('')
  const [unattendedInstall, setUnattendedInstall] = useState(false)
  const [adminPassword, setAdminPassword] = useState('')
  const [userLogin, setUserLogin] = useState('')
  const [userPassword, setUserPassword] = useState('')
  const [hostId, setHostId] = useState<string | null>(null)
  const [pools, setPools] = useState<Array<{ name: string }>>([])
  const [detectBusy, setDetectBusy] = useState(false)
  const [rhelToken, setRhelToken] = useState('')
  const [rhelBusy, setRhelBusy] = useState(false)

  const load = useCallback(async () => {
    setError(null)
    try {
      const [nets, hosts, storagePools] = await Promise.all([
        listPlatformNetworks().catch(() => []),
        listPlatformHosts().catch(() => []),
        listStoragePools().catch(() => []),
      ])
      setNetworks(nets)
      setPools(storagePools.map((p) => ({ name: p.name })))
      if (hosts.length > 0) setHostId(hosts[0].id)
      if (nets.length > 0) {
        setNetwork(nets[0].name)
        setPxeNetwork(nets[0].name)
      }
      if (storagePools.length > 0) setDiskPool(storagePools[0].name)
    } catch (e: unknown) {
      setError(formatUserError(e))
    }
  }, [])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    if (installSource === 'import') setStorageMode('existing')
  }, [installSource])

  useEffect(() => {
    const name = searchParams.get('name')
    const net = searchParams.get('network')
    if (name) setVmName(name)
    if (net) {
      setNetwork(net)
      setPxeNetwork(net)
    }
  }, [searchParams])

  const guestOptions = useMemo(
    () =>
      MACHINA_PACKER_SCRIPT_GUESTS.filter((g) => g.family !== 'windows').map((g) => ({
        id: g.virtInstallDownloadOs,
        label: g.label,
        osVariant: g.osVariantHint,
      })),
    [],
  )

  const specPreview = sizeToSpec(
    sizeState.size,
    sizeState.size === 'custom'
      ? { cores: sizeState.customCores, memoryGiB: sizeState.customMemoryGiB, diskGiB: sizeState.customDiskGiB }
      : undefined,
  )

  const canNext = () => {
    if (step === 0) {
      if (installSource === 'url') return locationUrl.trim().length > 0
      if (installSource === 'download') return installOs.trim().length > 0
      if (installSource === 'import') return true
      return true
    }
    if (step === 1) {
      const diskOk =
        installSource === 'import'
          ? existingDisk.trim().length > 0
          : storageMode === 'new'
        || (storageMode === 'existing' && existingDisk.trim().length > 0)
        || (storageMode === 'volume' && diskPool.trim().length > 0 && diskVol.trim().length > 0)
        || (storageMode === 'backing' && backingStore.trim().length > 0)
      return vmName.trim().length > 0 && sizeStepValid(sizeState) && diskOk
    }
    return true
  }

  const detectOsFromUrl = async () => {
    if (!hostId || !locationUrl.trim()) {
      toast.warning('Enter a URL and ensure a hypervisor is registered')
      return
    }
    setDetectBusy(true)
    try {
      const r = await queryHostLibvirt<{ stdout?: string; exit_code?: number; stderr?: string }>(
        hostId,
        'osinfo.detect',
        { url: locationUrl.trim() },
      )
      const parsed = parseOsinfoDetectVariant(r.stdout ?? '')
      if (parsed) {
        setOsVariant(parsed)
        toast.success(`osinfo-detect: applied “${parsed}”`)
      } else {
        toast.warning(r.stderr || 'Could not parse osinfo-detect output')
      }
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setDetectBusy(false)
    }
  }

  const finish = async () => {
    const spec = sizeToSpec(
      sizeState.size,
      sizeState.size === 'custom'
        ? {
            cores: sizeState.customCores,
            memoryGiB: sizeState.customMemoryGiB,
            diskGiB: sizeState.customDiskGiB,
          }
        : undefined,
    )
    const memoryGi = parseInt(spec.memory.replace(/Gi$/, ''), 10) || 4
    const diskGb = parseInt(spec.disk.replace(/Gi$/, ''), 10) || 40
    const body: Parameters<typeof createVmFromVirtInstall>[0] = {
      name: vmName.trim(),
      memory: `${memoryGi}Gi`,
      disk_gib: diskGb,
      network,
      os_variant: osVariant.trim() || undefined,
      firmware,
      host_id: hostId ?? undefined,
    }
    if (storageMode === 'existing') {
      body.existing_disk = existingDisk.trim()
      body.virt_install_path_in_use_check_off = true
    } else if (storageMode === 'volume') {
      body.root_disk_storage_pool = diskPool.trim()
      body.root_disk_storage_volume = diskVol.trim()
      body.virt_install_path_in_use_check_off = pathCheckOff || true
    } else if (storageMode === 'backing') {
      body.virt_install_disk_backing_store = backingStore.trim()
    }
    if (installIso.trim()) body.install_iso = installIso.trim()
    if (cloudInitUser.trim()) body.cloud_init_user = cloudInitUser.trim()
    if (cloudInitPassword) body.cloud_init_password = cloudInitPassword
    if (cloudInitSshKey.trim()) body.cloud_init_ssh_pubkey = cloudInitSshKey.trim()
    if (unattendedInstall || adminPassword || userLogin || userPassword) {
      body.virt_install_unattended = true
      if (adminPassword) body.virt_install_admin_password = adminPassword
      if (userLogin.trim()) body.virt_install_user_login = userLogin.trim()
      if (userPassword) body.virt_install_user_password = userPassword
    }
    if (installSource === 'url') {
      body.virt_install_location = locationUrl.trim()
      if (extraArgs.trim()) body.virt_install_extra_args = extraArgs.trim()
    } else if (installSource === 'pxe') {
      body.virt_install_pxe = true
      if (pxeNetwork.trim() && pxeNetwork !== network) {
        body.virt_install_pxe_network = pxeNetwork.trim()
      }
    } else if (installSource === 'download') {
      body.virt_install_install_os = installOs.trim()
    } else if (installSource === 'import') {
      body.existing_disk = existingDisk.trim()
      body.virt_install_define_only = true
    } else {
      body.virt_install_define_only = true
    }
    setBusy(true)
    try {
      const r = await createVmFromVirtInstall(body)
      toastQueuedOperation(toast, `Install VM ${vmName.trim()}`, r.task_id, tier)
      navigate('/platform/vms')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  const networkOptions =
    networks.length > 0
      ? networks.map((n) => ({ id: n.name, label: n.bridge ? `${n.name} (${n.bridge})` : n.name }))
      : [{ id: 'default', label: 'Default network (DHCP)' }]

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/vms" label="Virtual Machines" />}
      title="Advanced VM install"
      subtitle="PXE, kickstart URL, libosinfo download, or define-only shell — Cockpit Machines parity via virt-install."
    >
      <PlatformStepWizard
        open
        embedded
        onClose={() => navigate('/platform/vms')}
        title="Advanced VM install"
        steps={STEPS}
        step={step}
        onStepChange={setStep}
        canNext={canNext()}
        busy={busy}
        onFinish={finish}
        finishLabel="Create & install"
      >
        {step === 0 && (
          <MacGlassPanel title="Install source">
            <div className="grid gap-3 sm:grid-cols-2">
              {([
                ['download', 'Automatic OS install', 'virt-install --install os=… (libosinfo)'],
                ['url', 'URL / kickstart tree', 'virt-install --location http://…'],
                ['import', 'Import disk image', 'Define VM from existing qcow2/raw (--import)'],
                ['pxe', 'Network boot (PXE)', 'Extra NIC on libvirt network for PXE'],
                ['define', 'Define only', 'Halted shell — install media later'],
              ] as const).map(([id, label, hint]) => (
                <button
                  key={id}
                  type="button"
                  className={`text-left rounded-xl border p-4 transition ${installSource === id ? 'border-[var(--accent)]/50 bg-[var(--accent)]/10' : 'border-white/[0.08] hover:border-white/20'}`}
                  onClick={() => setInstallSource(id)}
                >
                  <p className="text-sm font-medium text-[var(--text-primary)]">{label}</p>
                  <p className="text-xs text-[var(--text-muted)] mt-1">{hint}</p>
                </button>
              ))}
            </div>
            {installSource === 'import' && (
              <p className="mt-4 text-xs text-[var(--text-muted)]">
                Choose the disk path in step 2 (Root disk source → existing path). OS variant above is used for libosinfo metadata.
              </p>
            )}
            {installSource === 'download' && (
              <div className="mt-4 space-y-2">
                <label className="text-xs text-[var(--text-muted)] block">OS profile (libosinfo short id)</label>
                <select aria-label="OS profile (libosinfo short id)" className="input w-full text-sm" value={installOs} onChange={(e) => {
                  setInstallOs(e.target.value)
                  const g = guestOptions.find((x) => x.id === e.target.value)
                  if (g?.osVariant) setOsVariant(g.osVariant)
                }}>
                  {guestOptions.map((g) => (
                    <option key={g.id} value={g.id}>{g.label} ({g.id})</option>
                  ))}
                </select>
              </div>
            )}
            {installSource === 'url' && (
              <div className="mt-4 space-y-3">
                <label className="text-xs text-[var(--text-muted)] block">Install tree URL or path</label>
                <input aria-label="Install tree URL or path" className="input w-full text-sm" value={locationUrl} onChange={(e) => setLocationUrl(e.target.value)} placeholder="http://mirror.example/os/ or /var/lib/libvirt/images/tree" />
                <button type="button" className="btn-secondary text-xs" disabled={detectBusy || !locationUrl.trim()} onClick={() => void detectOsFromUrl()}>
                  {detectBusy ? 'Detecting…' : 'Detect OS (osinfo-detect)'}
                </button>
                <label className="text-xs text-[var(--text-muted)] block">Extra kernel args (optional)</label>
                <input aria-label="Extra kernel args" className="input w-full text-sm font-mono" value={extraArgs} onChange={(e) => setExtraArgs(e.target.value)} placeholder="inst.ks=…" />
                <div className="rounded-lg border border-white/[0.06] p-3 space-y-2">
                  <p className="text-xs text-[var(--text-muted)]">RHEL image URL (RHSM offline token)</p>
                  <input aria-label="RHEL offline access token" className="input w-full text-sm" value={rhelToken} onChange={(e) => setRhelToken(e.target.value)} placeholder="offline access token" />
                  <button
                    type="button"
                    className="btn-secondary text-xs"
                    disabled={rhelBusy || !rhelToken.trim()}
                    data-testid="virt-install-rhel-resolve"
                    onClick={() => {
                      setRhelBusy(true)
                      void guestRhelImageUrl({ access_token: rhelToken.trim(), rhel_version: '9' })
                        .then((r) => {
                          const raw = r.raw as { href?: string; image?: { href?: string } } | undefined
                          const href = raw?.href ?? raw?.image?.href
                          if (href) {
                            setLocationUrl(href)
                            toast.success('RHEL image URL resolved')
                          } else {
                            toast.warning(r.error ?? 'No URL returned')
                          }
                        })
                        .catch((e: unknown) => toast.error(formatUserError(e)))
                        .finally(() => setRhelBusy(false))
                    }}
                  >
                    {rhelBusy ? 'Resolving…' : 'Resolve RHEL image URL'}
                  </button>
                </div>
              </div>
            )}
            {installSource === 'pxe' && (
              <p className="mt-4 text-xs text-[var(--text-muted)] flex items-start gap-2">
                <Network className="w-4 h-4 shrink-0 mt-0.5" />
                Primary NIC uses the network chosen in step 3; PXE NIC uses the PXE network below (defaults to the same).
              </p>
            )}
            <div className="mt-4 grid gap-3 sm:grid-cols-2">
              <label className="text-xs text-[var(--text-muted)]">
                OS variant
                <input className="input w-full text-sm mt-1" value={osVariant} onChange={(e) => setOsVariant(e.target.value)} placeholder="generic" />
              </label>
              <label className="text-xs text-[var(--text-muted)]">
                Firmware
                <select className="input w-full text-sm mt-1" value={firmware} onChange={(e) => setFirmware(e.target.value as 'bios' | 'uefi')}>
                  <option value="uefi">UEFI</option>
                  <option value="bios">BIOS</option>
                </select>
              </label>
            </div>
          </MacGlassPanel>
        )}

        {step === 1 && (
          <div className="space-y-4">
            <MacGlassPanel title="VM name">
              <input aria-label="VM name" className="input w-full text-sm" value={vmName} onChange={(e) => setVmName(e.target.value)} />
            </MacGlassPanel>
            <MacGlassPanel title="Root disk source">
              <div className="grid gap-2 sm:grid-cols-2 mb-3">
                {([
                  ['new', 'Create new qcow2'],
                  ['existing', 'Existing disk path'],
                  ['volume', 'Storage pool volume'],
                  ['backing', 'Overlay on golden image'],
                ] as const).map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    className={`text-left rounded-lg border px-3 py-2 text-sm ${storageMode === id ? 'border-[var(--accent)]/50 bg-[var(--accent)]/10' : 'border-white/[0.08]'}`}
                    onClick={() => setStorageMode(id)}
                  >
                    {label}
                  </button>
                ))}
              </div>
              {storageMode === 'existing' && (
                <input aria-label="Existing disk path" className="input w-full text-sm font-mono" value={existingDisk} onChange={(e) => setExistingDisk(e.target.value)} placeholder="/var/lib/libvirt/images/disk.qcow2" />
              )}
              {storageMode === 'volume' && (
                <div className="grid gap-2 sm:grid-cols-2">
                  <select aria-label="Storage pool" className="input text-sm" value={diskPool} onChange={(e) => setDiskPool(e.target.value)}>
                    {pools.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
                  </select>
                  <input aria-label="Volume name" className="input text-sm" value={diskVol} onChange={(e) => setDiskVol(e.target.value)} placeholder="volume name" />
                </div>
              )}
              {storageMode === 'backing' && (
                <input aria-label="Backing store path" className="input w-full text-sm font-mono" value={backingStore} onChange={(e) => setBackingStore(e.target.value)} placeholder="/var/lib/libvirt/images/golden.qcow2" />
              )}
              <label className="text-xs text-[var(--text-muted)] block mt-3">Install ISO path (optional)</label>
              <input aria-label="Install ISO path" className="input w-full text-sm font-mono mt-1" value={installIso} onChange={(e) => setInstallIso(e.target.value)} placeholder="/var/lib/libvirt/images/install.iso" />
            </MacGlassPanel>
            <MacGlassPanel title="Unattended / cloud-init (optional)">
              <label className="text-xs text-[var(--text-muted)] flex items-center gap-2 mb-3">
                <input type="checkbox" checked={unattendedInstall} onChange={(e) => setUnattendedInstall(e.target.checked)} />
                Use virt-install --unattended (when supported on hypervisor)
              </label>
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="text-xs text-[var(--text-muted)]">Admin password<input type="password" autoComplete="new-password" className="input w-full text-sm mt-1" value={adminPassword} onChange={(e) => setAdminPassword(e.target.value)} /></label>
                <label className="text-xs text-[var(--text-muted)]">User login<input className="input w-full text-sm mt-1" value={userLogin} onChange={(e) => setUserLogin(e.target.value)} /></label>
                <label className="text-xs text-[var(--text-muted)]">User password<input type="password" autoComplete="new-password" className="input w-full text-sm mt-1" value={userPassword} onChange={(e) => setUserPassword(e.target.value)} /></label>
                <label className="text-xs text-[var(--text-muted)]">Cloud-init user<input className="input w-full text-sm mt-1" value={cloudInitUser} onChange={(e) => setCloudInitUser(e.target.value)} /></label>
                <label className="text-xs text-[var(--text-muted)]">Cloud-init password<input type="password" autoComplete="new-password" className="input w-full text-sm mt-1" value={cloudInitPassword} onChange={(e) => setCloudInitPassword(e.target.value)} /></label>
              </div>
              <label className="text-xs text-[var(--text-muted)] block mt-3">SSH public key</label>
              <textarea aria-label="SSH public key" className="input w-full text-sm font-mono mt-1 min-h-[4rem]" value={cloudInitSshKey} onChange={(e) => setCloudInitSshKey(e.target.value)} />
            </MacGlassPanel>
            <VmWizardSizeStep state={sizeState} onChange={(patch) => setSizeState((s) => ({ ...s, ...patch }))} />
          </div>
        )}

        {step === 2 && (
          <MacGlassPanel title="Networking">
            <label className="text-xs text-[var(--text-muted)] block mb-1">Primary libvirt network</label>
            <select aria-label="Primary libvirt network" className="input w-full text-sm mb-4" value={network} onChange={(e) => setNetwork(e.target.value)}>
              {networkOptions.map((n) => (
                <option key={n.id} value={n.id}>{n.label}</option>
              ))}
            </select>
            {installSource === 'pxe' && (
              <>
                <label className="text-xs text-[var(--text-muted)] block mb-1">PXE network</label>
                <select aria-label="PXE network" className="input w-full text-sm" value={pxeNetwork} onChange={(e) => setPxeNetwork(e.target.value)}>
                  {networkOptions.map((n) => (
                    <option key={n.id} value={n.id}>{n.label}</option>
                  ))}
                </select>
              </>
            )}
          </MacGlassPanel>
        )}

        {step === 3 && (
          <MacGlassPanel title="Review">
            <ul className="text-sm text-[var(--text-secondary)] space-y-2">
              <li><span className="text-[var(--text-muted)]">Name:</span> {vmName}</li>
              <li><span className="text-[var(--text-muted)]">Source:</span> {installSource}</li>
              <li><span className="text-[var(--text-muted)]">Size:</span> {specPreview.cores} vCPU · {specPreview.memory} · {specPreview.disk}</li>
              <li><span className="text-[var(--text-muted)]">Network:</span> {network}{installSource === 'pxe' ? ` (PXE: ${pxeNetwork})` : ''}</li>
              <li><span className="text-[var(--text-muted)]">Firmware:</span> {firmware.toUpperCase()}</li>
            </ul>
            <p className="text-xs text-[var(--text-muted)] mt-4 flex items-center gap-2">
              <Globe className="w-4 h-4" />
              virt-install runs on the assigned hypervisor; monitor progress in Tasks.
            </p>
          </MacGlassPanel>
        )}
      </PlatformStepWizard>
      <p className="text-xs text-[var(--text-muted)] mt-4 text-center">
        Prefer a library ISO? <Link to="/platform/create-iso" className="text-[var(--link)] hover:underline">Create from ISO</Link>
      </p>
    </PlatformPageChrome>
  )
}
