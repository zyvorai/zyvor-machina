// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { Upload } from 'lucide-react'
import {
  getTemplateReadiness,
  listPlatformHosts,
  listPlatformNetworks,
  listMarketplaceTemplates,
  seedDefaultTemplates,
  type PlatformHost,
  type PlatformNetwork,
  type PlatformTemplate,
} from '../../api/platform'
import { readSshPubkeyFile } from '../../utils/sshPubkeyImport'
import PlatformStepWizard from './PlatformStepWizard'
import VmWizardReadinessBanner from './VmWizardReadinessBanner'
import type { TemplateReadiness } from '../../api/platform'
import { usePlatformInfo } from '../../contexts/PlatformInfoContext'
import VmWizardSizeStep, { sizeStepValid, type VmWizardSizeState } from './VmWizardSizeStep'
import {
  buildOsFlavorList,
  cloudInitUserForOs,
  findTemplate,
  OS_CATEGORIES,
  osFlavorById,
  sizeToSpec,
  WIZARD_STEPS,
  type OsFlavor,
} from './vmWizardCatalog'

export type { VmSpecNumbers } from './vmWizardCatalog'
export { sizeToSpec, cloudInitUserForOs, buildOsFlavorList } from './vmWizardCatalog'

export interface VmWizardInitial {
  name?: string
  os?: string
  size?: string
  network?: string
  hostId?: string
}

interface VmWizardWindowsOptions {
  virtio: boolean
  virtioIsoPath: string
  uefi: boolean
  tpm: boolean
  secureBoot: boolean
}

export interface VmWizardPayload {
  name: string
  os: string
  size: string
  network: string
  cloudInitSshPubkey?: string
  customSpec?: { cores: number; memoryGiB: number; diskGiB: number }
  graphicsType?: 'vnc' | 'spice' | 'both'
  graphicsListen?: string
  windows?: VmWizardWindowsOptions
  templateVersion?: string
  hostId?: string
  /** When true, parent should call createFromTemplate */
  fromTemplate?: boolean
}

interface SimpleCreateVmWizardProps {
  open: boolean
  onClose: () => void
  onCreate: (payload: VmWizardPayload) => Promise<void>
  initial?: VmWizardInitial
}

function categoryAccent(category: OsFlavor['category']): string {
  switch (category) {
    case 'Windows':
      return 'from-[color-mix(in_srgb,var(--accent)_20%,transparent)] to-[var(--apple-surface)] border-[var(--accent)]/40'
    case 'Database':
      return 'from-emerald-600/20 to-[var(--apple-surface)] border-emerald-500/35'
    case 'Appliance':
      return 'from-amber-600/15 to-[var(--apple-surface)] border-amber-500/35'
    case 'Special':
      return 'from-violet-600/20 to-[var(--apple-surface)] border-violet-500/35'
    default:
      return 'from-orange-600/15 to-[var(--apple-surface)] border-[var(--apple-hairline)]'
  }
}

export default function SimpleCreateVmWizard({ open, onClose, onCreate, initial }: SimpleCreateVmWizardProps) {
  const { info } = usePlatformInfo()
  const [step, setStep] = useState(0)
  // Not 'new-vm': that name is a near-certain collision — the fleet already
  // has a VM literally named "new-vm" on this deployment, and it's the kind
  // of generic default any repeat wizard run would reuse. An empty default
  // forces a deliberate name instead of a blind click-through to a doomed
  // "Create VM" submission.
  const [name, setName] = useState('')
  const [os, setOs] = useState('ubuntu-24.04')
  const [osFilter, setOsFilter] = useState<(typeof OS_CATEGORIES)[number]>('All')
  const [sizeState, setSizeState] = useState<VmWizardSizeState>({
    size: 'medium',
    customCores: 4,
    customMemoryGiB: 8,
    customDiskGiB: 80,
  })
  const [network, setNetwork] = useState('default')
  const [graphicsType, setGraphicsType] = useState<'vnc' | 'spice' | 'both'>('both')
  const [graphicsListen, setGraphicsListen] = useState('127.0.0.1')
  const [sshPubkey, setSshPubkey] = useState('')
  const [busy, setBusy] = useState(false)
  const [templates, setTemplates] = useState<PlatformTemplate[]>([])
  const [networks, setNetworks] = useState<PlatformNetwork[]>([])
  const [hosts, setHosts] = useState<PlatformHost[]>([])
  const [hostId, setHostId] = useState<string>('')
  const [readinessLoading, setReadinessLoading] = useState(false)
  const [readiness, setReadiness] = useState<TemplateReadiness | null>(null)
  const [virtio, setVirtio] = useState(true)
  const [virtioIsoPath, setVirtioIsoPath] = useState('/var/lib/libvirt/images/isos/virtio-win.iso')
  const [uefi, setUefi] = useState(true)
  const [tpm, setTpm] = useState(true)
  const [secureBoot, setSecureBoot] = useState(true)
  const pubkeyFileRef = useRef<HTMLInputElement>(null)

  const loadCatalog = useCallback(async () => {
    try {
      let tpls = await listMarketplaceTemplates()
      if (tpls.length === 0) {
        const seeded = await seedDefaultTemplates()
        tpls = seeded.templates
      }
      setTemplates(tpls)
      const [nets, hostList] = await Promise.all([
        listPlatformNetworks(),
        listPlatformHosts().catch(() => [] as PlatformHost[]),
      ])
      setNetworks(nets)
      setHosts(hostList.filter((h) => h.state === 'online'))
      // Functional update so this callback doesn't close over `network` — a
      // `network` dep made loadCatalog unstable, which re-ran the reset effect
      // below and threw the user back to step 0 (and cleared the placement host)
      // whenever they touched the Network select on the final step.
      setNetwork((cur) => (nets.length > 0 && !nets.some((n) => n.name === cur) ? nets[0].name : cur))
    } catch {
      setTemplates([])
      setNetworks([])
      setHosts([])
    }
  }, [])

  useEffect(() => {
    if (!open) return
    setStep(0)
    if (initial?.name) setName(initial.name)
    if (initial?.os) setOs(initial.os)
    if (initial?.size) setSizeState((s) => ({ ...s, size: initial.size! }))
    if (initial?.network) setNetwork(initial.network)
    if (initial?.hostId) setHostId(initial.hostId)
    else setHostId('')
    void loadCatalog()
  }, [open, initial, loadCatalog])

  const flavors = useMemo(() => buildOsFlavorList(templates), [templates])
  const filteredOs = useMemo(() => {
    if (osFilter === 'All') return flavors
    return flavors.filter((o) => o.category === osFilter)
  }, [flavors, osFilter])

  const selectedFlavor = flavors.find((f) => f.id === os) ?? osFlavorById(os)
  const matchedTemplate = findTemplate(templates, os)
  const isWindows = selectedFlavor?.windows ?? os.startsWith('windows')
  const isCustomIso = os === 'custom-iso'
  const isCustomVirtInstall = os === 'custom-virt-install'
  const needsReadiness = Boolean(matchedTemplate) && !isWindows && !isCustomIso

  useEffect(() => {
    if (!open || !needsReadiness || !matchedTemplate) {
      setReadiness(null)
      return
    }
    let cancelled = false
    setReadinessLoading(true)
    void getTemplateReadiness(matchedTemplate.name, matchedTemplate.version)
      .then((r) => {
        if (!cancelled) setReadiness(r)
      })
      .catch(() => {
        if (!cancelled) setReadiness(null)
      })
      .finally(() => {
        if (!cancelled) setReadinessLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open, os, needsReadiness, matchedTemplate?.name, matchedTemplate?.version])

  const specPreview = sizeToSpec(
    sizeState.size,
    sizeState.size === 'custom'
      ? {
          cores: sizeState.customCores,
          memoryGiB: sizeState.customMemoryGiB,
          diskGiB: sizeState.customDiskGiB,
        }
      : undefined,
  )

  const networkOptions = useMemo(() => {
    if (networks.length === 0) {
      return [
        { id: 'default', label: 'Default network (DHCP)' },
        { id: 'prod', label: 'Production VLAN' },
        { id: 'isolated', label: 'Isolated lab' },
      ]
    }
    return networks.map((n) => ({
      id: n.name,
      label: n.bridge ? `${n.name} (${n.bridge})` : n.name,
    }))
  }, [networks])

  const canNext = () => {
    if (step === 0) return name.trim().length > 0
    if (step === 1) return Boolean(os)
    if (step === 2) return sizeStepValid(sizeState)
    if (step === 3 && needsReadiness && readiness && !readiness.ready) return false
    return true
  }

  const submit = async () => {
    setBusy(true)
    try {
      const key = sshPubkey.trim()
      const payload: VmWizardPayload = {
        name: name.trim(),
        os,
        size: sizeState.size,
        network,
        cloudInitSshPubkey: key || undefined,
        customSpec:
          sizeState.size === 'custom'
            ? {
                cores: sizeState.customCores,
                memoryGiB: sizeState.customMemoryGiB,
                diskGiB: sizeState.customDiskGiB,
              }
            : undefined,
        templateVersion: matchedTemplate?.version,
        hostId: hostId || undefined,
        fromTemplate: needsReadiness,
        graphicsType,
        graphicsListen,
      }
      if (isWindows) {
        payload.windows = { virtio, virtioIsoPath, uefi, tpm, secureBoot: secureBoot && uefi }
      }
      await onCreate(payload)
      onClose()
    } finally {
      setBusy(false)
    }
  }

  return (
    <PlatformStepWizard
      open={open}
      onClose={onClose}
      title="Create Virtual Machine"
      subtitle="Choose OS, size, and network — guided setup."
      steps={[...WIZARD_STEPS]}
      step={step}
      onStepChange={setStep}
      canNext={canNext()}
      busy={busy}
      finishLabel="Create VM"
      onFinish={submit}
    >
      {step === 0 && (
        <div className="space-y-4">
          <label className="block text-sm">
            <span className="text-[var(--text-secondary)]">Virtual machine name</span>
            <input
              className="input w-full mt-1 text-base"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="my-app-server"
              autoFocus
            />
          </label>
          <p className="text-xs text-[var(--text-muted)]">
            Use lowercase letters, numbers, and hyphens. Hostname for cloud-init defaults to this name.
          </p>
        </div>
      )}

      {step === 1 && (
        <div className="space-y-4">
          <div className="flex flex-wrap gap-1.5">
            {OS_CATEGORIES.map((cat) => (
              <button
                key={cat}
                type="button"
                className={`px-2.5 py-1 rounded-lg text-xs border transition-colors ${
                  osFilter === cat
                    ? 'border-[var(--accent)]/60 bg-[var(--accent-soft)] text-[var(--text-primary)]'
                    : 'border-[var(--apple-hairline)] text-[var(--text-muted)] hover:border-[var(--apple-hairline)]'
                }`}
                onClick={() => setOsFilter(cat)}
              >
                {cat}
              </button>
            ))}
          </div>
          {templates.length === 0 && (
            <p className="text-xs text-amber-700/80">
              Template catalog loading failed — showing built-in flavors.{' '}
              <Link to="/platform/templates" className="underline">
                Seed templates
              </Link>
            </p>
          )}
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3 pr-1">
            {filteredOs.map((flavor) => {
              const selected = os === flavor.id
              return (
                <button
                  key={flavor.id}
                  type="button"
                  className={`text-left p-3 rounded-xl border bg-gradient-to-br transition-all ${
                    selected ? 'ring-2 ring-[color-mix(in_srgb,var(--accent)_55%,transparent)] border-[var(--accent)]/50' : 'hover:border-[var(--apple-hairline)]'
                  } ${categoryAccent(flavor.category)}`}
                  onClick={() => setOs(flavor.id)}
                >
                  <div className="flex items-start gap-2">
                    <span className="text-2xl leading-none" aria-hidden>
                      {flavor.icon}
                    </span>
                    <div className="min-w-0 flex-1">
                      <p className="font-medium text-[var(--text-primary)] text-sm truncate">{flavor.label}</p>
                      <p className="text-[11px] text-[var(--text-muted)] mt-0.5 line-clamp-2">{flavor.subtitle}</p>
                    </div>
                  </div>
                </button>
              )
            })}
          </div>
          {needsReadiness && matchedTemplate && (
            <VmWizardReadinessBanner
              loading={readinessLoading}
              readiness={readiness}
              templateName={matchedTemplate.name}
              templateVersion={matchedTemplate.version}
              onReadinessChange={setReadiness}
            />
          )}
          {isCustomIso && (
            <p className="text-xs text-amber-700/80 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2">
              Custom ISO opens the dedicated install wizard when you finish this flow.
            </p>
          )}
        </div>
      )}

      {step === 2 && (
        <div className="space-y-4">
          <VmWizardSizeStep state={sizeState} onChange={(patch) => setSizeState((s) => ({ ...s, ...patch }))} />
          {hosts.length > 0 && (
            <label className="block text-sm">
              <span className="text-[var(--text-secondary)]">Placement host (optional)</span>
              <select className="input w-full mt-1" value={hostId} onChange={(e) => setHostId(e.target.value)}>
                <option value="">Automatic — controller picks host</option>
                {hosts.map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.hostname}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
      )}

      {step === 3 && (
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
          {networks.length === 0 && (
            <p className="text-xs text-[var(--text-muted)]">
              No platform networks yet.{' '}
              <Link to="/platform/networks" className="underline">
                Discover networks
              </Link>{' '}
              from libvirt, or use the default name.
            </p>
          )}

          {isWindows && (
            <div className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-3 space-y-2 text-sm">
              <p className="text-[var(--text-secondary)] font-medium">Windows options</p>
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={virtio} onChange={(e) => setVirtio(e.target.checked)} /> VirtIO drivers
              </label>
              {virtio && (
                <label className="block pl-6">
                  <span className="text-[var(--text-muted)] text-xs">VirtIO ISO path on hypervisor</span>
                  <input
                    className="input w-full mt-1 text-xs font-mono"
                    value={virtioIsoPath}
                    onChange={(e) => setVirtioIsoPath(e.target.value)}
                    placeholder="/var/lib/libvirt/images/isos/virtio-win.iso"
                  />
                </label>
              )}
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={uefi} onChange={(e) => setUefi(e.target.checked)} /> UEFI firmware
              </label>
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={tpm} onChange={(e) => setTpm(e.target.checked)} /> TPM
              </label>
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={secureBoot && uefi}
                  disabled={!uefi}
                  onChange={(e) => setSecureBoot(e.target.checked)}
                />{' '}
                <span className={uefi ? undefined : 'text-[var(--text-muted)]'}>Secure Boot</span>
              </label>
              {!uefi && (
                <p className="pl-6 text-xs text-[var(--text-muted)]">Secure Boot requires UEFI firmware.</p>
              )}
            </div>
          )}

          {!isCustomIso && !isCustomVirtInstall && (
            <div className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-3 space-y-3 text-sm">
              <p className="text-[var(--text-secondary)] font-medium">Console graphics</p>
              <label className="block">
                <span className="text-[var(--text-muted)] text-xs">Protocol</span>
                <select
                  className="input w-full mt-1"
                  value={graphicsType}
                  onChange={(e) => setGraphicsType(e.target.value as 'vnc' | 'spice' | 'both')}
                >
                  <option value="both">VNC + SPICE (recommended)</option>
                  <option value="vnc">VNC only</option>
                  <option value="spice">SPICE only</option>
                </select>
              </label>
              <label className="block">
                <span className="text-[var(--text-muted)] text-xs">Listen address</span>
                <select
                  className="input w-full mt-1"
                  value={graphicsListen}
                  onChange={(e) => setGraphicsListen(e.target.value)}
                >
                  <option value="127.0.0.1">127.0.0.1 (local / SSH tunnel)</option>
                  <option value="0.0.0.0">0.0.0.0 (all interfaces)</option>
                  <option value="::1">::1 (IPv6 localhost)</option>
                </select>
              </label>
            </div>
          )}

          {!isWindows && !isCustomIso && (
            <div className="rounded-xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 p-3 space-y-3 text-sm">
              <label className="block">
                <span className="text-[var(--text-secondary)]">SSH public key (optional)</span>
                <textarea
                  className="input w-full mt-1 font-mono text-xs min-h-[4rem]"
                  placeholder="ssh-ed25519 AAAA… user@host"
                  value={sshPubkey}
                  onChange={(e) => setSshPubkey(e.target.value)}
                />
              </label>
              <div className="flex flex-wrap items-center gap-2">
                <input
                  ref={pubkeyFileRef}
                  type="file"
                  accept=".pub,text/plain"
                  className="hidden"
                  onChange={(e) => readSshPubkeyFile(e.target.files?.[0], setSshPubkey)}
                />
                <button
                  type="button"
                  className="btn-secondary text-xs inline-flex items-center gap-1"
                  onClick={() => pubkeyFileRef.current?.click()}
                >
                  <Upload className="w-3 h-3" /> Import public key (.pub)
                </button>
              </div>
              <p className="text-xs text-[var(--text-muted)]">
                Cloud-init user: {cloudInitUserForOs(os)}. Machina does not store private keys.
              </p>
            </div>
          )}

          {needsReadiness && matchedTemplate && (
            <VmWizardReadinessBanner
              loading={readinessLoading}
              readiness={readiness}
              templateName={matchedTemplate.name}
              templateVersion={matchedTemplate.version}
              onReadinessChange={setReadiness}
            />
          )}
          {!info?.guestkit?.enabled && (
            <p className="text-xs text-orange-700/80 rounded-lg border border-orange-500/25 bg-orange-500/10 px-3 py-2">
              Heads up: offline disk assurance (GuestKit) isn't enabled on this controller, so this VM's
              disk won't be scored for migration readiness until an administrator turns it on. Creation
              is unaffected.
            </p>
          )}

          <div className="rounded-2xl border border-[var(--apple-hairline)] bg-[var(--apple-surface)]/50 bg-[var(--apple-surface)]/80 p-4 text-sm space-y-1.5">
            <p className="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)] mb-2">Review</p>
            <p>
              <span className="text-[var(--text-muted)]">Name:</span> <span className="text-[var(--text-primary)]">{name.trim()}</span>
            </p>
            <p>
              <span className="text-[var(--text-muted)]">OS:</span>{' '}
              <span className="text-[var(--text-primary)]">{selectedFlavor?.label ?? os}</span>
              {matchedTemplate && (
                <span className="text-[var(--text-muted)] text-xs"> ({matchedTemplate.name}@{matchedTemplate.version})</span>
              )}
            </p>
            <p>
              <span className="text-[var(--text-muted)]">Size:</span>{' '}
              <span className="text-[var(--text-primary)]">
                {specPreview.cores} vCPU · {specPreview.memory} RAM
                {needsReadiness ? '' : ` · ${specPreview.disk} disk`}
              </span>
            </p>
            <p>
              <span className="text-[var(--text-muted)]">Network:</span>{' '}
              <span className="text-[var(--text-primary)]">{networkOptions.find((n) => n.id === network)?.label ?? network}</span>
            </p>
            {!isCustomIso && !isCustomVirtInstall && (
              <p>
                <span className="text-[var(--text-muted)]">Console:</span>{' '}
                <span className="text-[var(--text-primary)]">{graphicsType} @ {graphicsListen}</span>
              </p>
            )}
            {hostId && (
              <p>
                <span className="text-[var(--text-muted)]">Host:</span>{' '}
                <span className="text-[var(--text-primary)]">{hosts.find((h) => h.id === hostId)?.hostname ?? hostId}</span>
              </p>
            )}
          </div>
        </div>
      )}
    </PlatformStepWizard>
  )
}
