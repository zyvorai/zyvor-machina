// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { Disc, Upload } from 'lucide-react'
import PlatformPageChrome, { PlatformBackLink } from '../../components/platform/PlatformPageChrome'
import PlatformStepWizard from '../../components/platform/PlatformStepWizard'
import VmWizardSizeStep, { sizeStepValid, type VmWizardSizeState } from '../../components/platform/VmWizardSizeStep'
import { MacGlassPanel } from '../../components/platform/mac/PlatformMacUi'
import PlatformEmptyState from '../../components/platform/PlatformEmptyState'
import {
  createVmFromIso,
  listContentImages,
  listPlatformNetworks,
  type ContentImage,
} from '../../api/platform'
import { sizeToSpec } from '../../components/platform/vmWizardCatalog'
import { readSshPubkeyFile } from '../../utils/sshPubkeyImport'
import { useToastContext } from '../../contexts/ToastContext'
import { usePlatformDesktopTier } from '../../hooks/usePlatformDesktopTier'
import { formatUserError } from '../../utils/apiError'
import { toastQueuedOperation } from '../../utils/platformTaskToast'
import { hubLinkClasses } from '../../utils/semanticColors'

const ISO_STEPS = ['ISO image', 'Name & size', 'Network & access', 'Review']

export default function PlatformIsoCreate() {
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const toast = useToastContext()
  const [tier] = usePlatformDesktopTier()
  const [step, setStep] = useState(0)
  const [images, setImages] = useState<ContentImage[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [isoPath, setIsoPath] = useState('')
  const [vmName, setVmName] = useState('vm-from-iso')
  const [sizeState, setSizeState] = useState<VmWizardSizeState>({
    size: 'medium',
    customCores: 4,
    customMemoryGiB: 8,
    customDiskGiB: 40,
  })
  const [network, setNetwork] = useState('default')
  const [networks, setNetworks] = useState<{ name: string; bridge?: string | null }[]>([])
  const [sshPubkey, setSshPubkey] = useState('')

  const approved = useMemo(
    () => images.filter((i) => i.status === 'available' && (i.kind === 'iso' || i.path.toLowerCase().endsWith('.iso'))),
    [images],
  )

  const load = useCallback(async () => {
    setError(null)
    setLoading(true)
    try {
      const [imgs, nets] = await Promise.all([
        listContentImages({ status: 'available' }),
        listPlatformNetworks().catch(() => []),
      ])
      setImages(imgs)
      setNetworks(nets)
      if (nets.length > 0) setNetwork(nets[0].name)
    } catch (e: unknown) {
      setError(formatUserError(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { void load() }, [load])

  useEffect(() => {
    const pre = searchParams.get('iso_path') ?? searchParams.get('iso')
    const name = searchParams.get('name')
    if (pre) setIsoPath(pre)
    if (name) setVmName(name)
  }, [searchParams])

  const selected = approved.find((i) => i.path === isoPath)
  const specPreview = sizeToSpec(
    sizeState.size,
    sizeState.size === 'custom'
      ? { cores: sizeState.customCores, memoryGiB: sizeState.customMemoryGiB, diskGiB: sizeState.customDiskGiB }
      : undefined,
  )

  const networkOptions =
    networks.length > 0
      ? networks.map((n) => ({ id: n.name, label: n.bridge ? `${n.name} (${n.bridge})` : n.name }))
      : [{ id: 'default', label: 'Default network (DHCP)' }]

  const canNext = () => {
    if (step === 0) return Boolean(isoPath.trim())
    if (step === 1) return vmName.trim().length > 0 && sizeStepValid(sizeState)
    return true
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
    setBusy(true)
    try {
      const r = await createVmFromIso({
        name: vmName.trim(),
        iso_path: isoPath,
        memory: `${memoryGi}Gi`,
        disk_gib: diskGb,
        cloud_init_ssh_pubkey: sshPubkey.trim() || undefined,
      })
      toastQueuedOperation(toast, `ISO install for ${vmName.trim()}`, r.task_id, tier)
      navigate('/platform/vms')
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PlatformPageChrome
      eyebrow="Platform"
      error={error}
      onErrorRetry={() => void load()}
      prepend={<PlatformBackLink to="/platform/content" label="Content Library" />}
      title="Create VM from ISO"
      subtitle="Guided install from approved Content Library images."
      contentLoading={loading}
    >
      {!loading && approved.length === 0 ? (
        <PlatformEmptyState
          title="No approved ISOs"
          subtitle="Upload and approve an ISO in the Content Library first."
          action={
            <Link to="/platform/content" className="btn-primary text-sm">
              Open Content Library
            </Link>
          }
        />
      ) : (
        <PlatformStepWizard
          open
          embedded
          onClose={() => navigate('/platform/content')}
          title="Install from ISO"
          steps={ISO_STEPS}
          step={step}
          onStepChange={setStep}
          canNext={canNext()}
          busy={busy}
          finishLabel="Create VM"
          onFinish={finish}
        >
          {step === 0 && (
            <div className="grid gap-2 sm:grid-cols-2">
              {approved.map((img) => (
                <button
                  key={img.id}
                  type="button"
                  className={`text-left rounded-xl border px-3 py-2 text-sm ${
                    isoPath === img.path ? 'border-[var(--accent)]/50 bg-[var(--accent)]/10' : 'border-white/10 hover:border-white/20'
                  }`}
                  onClick={() => setIsoPath(img.path)}
                >
                  <span className="font-medium flex items-center gap-2">
                    <Disc className="w-4 h-4 shrink-0" />
                    {img.name}
                  </span>
                  <span className="text-xs text-[var(--text-muted)] font-mono block mt-0.5 truncate">{img.path}</span>
                </button>
              ))}
            </div>
          )}
          {step === 1 && (
            <div className="space-y-4">
              <label className="block text-sm">
                <span className="text-[var(--text-secondary)]">VM name</span>
                <input className="input w-full mt-1" value={vmName} onChange={(e) => setVmName(e.target.value)} />
              </label>
              <VmWizardSizeStep state={sizeState} onChange={(patch) => setSizeState((s) => ({ ...s, ...patch }))} />
            </div>
          )}
          {step === 2 && (
            <div className="space-y-4">
              <label className="block text-sm">
                <span className="text-[var(--text-secondary)]">Network</span>
                <select className="input w-full mt-1" value={network} onChange={(e) => setNetwork(e.target.value)}>
                  {networkOptions.map((n) => (
                    <option key={n.id} value={n.id}>
                      {n.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block text-sm">
                <span className="text-[var(--text-secondary)]">SSH public key (optional)</span>
                <textarea
                  className="input w-full mt-1 font-mono text-xs min-h-[4rem]"
                  value={sshPubkey}
                  onChange={(e) => setSshPubkey(e.target.value)}
                  placeholder="ssh-ed25519 AAAA…"
                />
              </label>
              <button
                type="button"
                className="btn-secondary text-xs inline-flex items-center gap-1"
                onClick={() => {
                  const input = document.createElement('input')
                  input.type = 'file'
                  input.accept = '.pub,text/plain'
                  input.onchange = () => readSshPubkeyFile(input.files?.[0], setSshPubkey)
                  input.click()
                }}
              >
                <Upload className="w-3 h-3" /> Import .pub
              </button>
            </div>
          )}
          {step === 3 && (
            <MacGlassPanel title="Review" subtitle={selected?.name}>
              <ul className="text-sm space-y-1 text-[var(--text-secondary)]">
                <li>
                  ISO: <span className="font-mono text-xs">{isoPath}</span>
                </li>
                <li>Name: {vmName}</li>
                <li>
                  Size: {specPreview.cores} vCPU · {specPreview.memory} · {specPreview.disk} disk
                </li>
                <li>Network: {network}</li>
              </ul>
              <button type="button" className={`btn-secondary text-xs mt-3 ${hubLinkClasses()}`} onClick={() => {
                const spec = sizeToSpec(sizeState.size, sizeState.size === 'custom' ? {
                  cores: sizeState.customCores,
                  memoryGiB: sizeState.customMemoryGiB,
                  diskGiB: sizeState.customDiskGiB,
                } : undefined)
                const q = new URLSearchParams({
                  iso_path: isoPath,
                  name: vmName.trim(),
                  vcpus: String(spec.cores),
                  memory_mb: String((parseInt(spec.memory.replace(/Gi$/, ''), 10) || 4) * 1024),
                  disk_gb: String(parseInt(spec.disk.replace(/Gi$/, ''), 10) || 40),
                })
                navigate(`/create?${q.toString()}`)
              }}>
                Advanced: classic virt-install wizard
              </button>
            </MacGlassPanel>
          )}
        </PlatformStepWizard>
      )}
    </PlatformPageChrome>
  )
}
