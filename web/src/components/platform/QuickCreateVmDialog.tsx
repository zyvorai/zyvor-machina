// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { GlassModal } from '../glass/GlassModal'
import {
  getTemplateReadiness,
  listMarketplaceTemplates,
  listPlatformNetworks,
  seedDefaultTemplates,
  type PlatformNetwork,
  type PlatformTemplate,
  type TemplateReadiness,
} from '../../api/platform'
import { formatUserError } from '../../utils/apiError'
import { useToastContext } from '../../contexts/ToastContext'
import VmWizardReadinessBanner from './VmWizardReadinessBanner'
import type { VmWizardPayload } from './SimpleCreateVmWizard'
import { SIZE_PRESETS, buildOsFlavorList, findTemplate, type OsFlavor } from './vmWizardCatalog'

/** The images most people want, in the order they should appear. */
const QUICK_IMAGE_ORDER = ['ubuntu-24.04', 'debian-13', 'rocky-10', 'fedora-44', 'alma-10', 'win11', 'k8s-node', 'postgresql-16']
const QUICK_SIZES = SIZE_PRESETS.filter((s) => s.id !== 'xlarge')
/** Short, readable default name: "ubuntu-24-04-7k2f". */
function suggestName(osId: string): string {
  const base = osId.replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '').toLowerCase() || 'vm'
  return `${base}-${Math.random().toString(36).slice(2, 6)}`
}

/**
 * Create a VM in three taps: pick an image, pick a size, press Create. Network and everything else take the
 * host's defaults; "More options" opens the full wizard for anything unusual (custom ISO, PXE, GPU, Windows tuning).
 */
export default function QuickCreateVmDialog({
  open,
  onClose,
  onCreate,
  onAdvanced,
}: {
  open: boolean
  onClose: () => void
  onCreate: (payload: VmWizardPayload) => Promise<void>
  onAdvanced: () => void
}) {
  const toast = useToastContext()
  const [templates, setTemplates] = useState<PlatformTemplate[]>([])
  const [networks, setNetworks] = useState<PlatformNetwork[]>([])
  const [os, setOs] = useState('ubuntu-24.04')
  const [size, setSize] = useState('medium')
  const [name, setName] = useState('')
  const [suggested, setSuggested] = useState(() => suggestName('ubuntu-24.04'))
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(false)
  const [readiness, setReadiness] = useState<TemplateReadiness | null>(null)
  const [readinessLoading, setReadinessLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      let tpls = await listMarketplaceTemplates()
      if (tpls.length === 0) tpls = (await seedDefaultTemplates()).templates
      setTemplates(tpls)
      setNetworks(await listPlatformNetworks().catch(() => [] as PlatformNetwork[]))
    } catch {
      setTemplates([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!open) return
    setName('')
    setSuggested(suggestName(os))
    void load()
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reset once per open
  }, [open, load])

  const picks = useMemo<OsFlavor[]>(() => {
    const all = buildOsFlavorList(templates)
    const byId = new Map(all.map((f) => [f.id, f]))
    return QUICK_IMAGE_ORDER.map((id) => byId.get(id)).filter((f): f is OsFlavor => Boolean(f))
  }, [templates])

  const flavor = picks.find((p) => p.id === os)
  const isWindows = Boolean(flavor?.windows)
  const matched = findTemplate(templates, os)
  const needsReadiness = Boolean(matched) && !isWindows

  useEffect(() => {
    if (!open || !needsReadiness || !matched) {
      setReadiness(null)
      return
    }
    let cancelled = false
    setReadinessLoading(true)
    void getTemplateReadiness(matched.name, matched.version)
      .then((r) => { if (!cancelled) setReadiness(r) })
      .catch(() => { if (!cancelled) setReadiness(null) })
      .finally(() => { if (!cancelled) setReadinessLoading(false) })
    return () => { cancelled = true }
  }, [open, needsReadiness, matched?.name, matched?.version]) // eslint-disable-line react-hooks/exhaustive-deps

  const network = networks[0]?.name ?? 'default'
  const finalName = (name.trim() || suggested).toLowerCase().replace(/[^a-z0-9-_.]+/g, '-')
  const blocked = needsReadiness && readiness != null && !readiness.ready

  const create = async () => {
    setBusy(true)
    try {
      const payload: VmWizardPayload = {
        name: finalName,
        os,
        size,
        network,
        templateVersion: matched?.version,
        fromTemplate: needsReadiness,
        ...(isWindows ? { windows: { virtio: true, virtioIsoPath: '/var/lib/libvirt/images/isos/virtio-win.iso', uefi: true, tpm: true, secureBoot: true } } : {}),
      }
      await onCreate(payload)
      onClose()
    } catch (e: unknown) {
      toast.error(formatUserError(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <GlassModal open={open} onClose={onClose} wide title="New virtual machine" subtitle="Pick an image and a size. Everything else is set for you.">
      <div className="space-y-5" data-testid="quick-create-vm">
        <div>
          <p className="mb-2 text-xs font-semibold uppercase tracking-[0.12em] text-[var(--text-muted)]">Image</p>
          {loading && picks.length === 0 ? (
            <p className="flex items-center gap-2 text-sm text-[var(--text-muted)]"><Loader2 className="h-4 w-4 animate-spin" /> Loading images…</p>
          ) : (
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3" role="radiogroup" aria-label="Image">
              {picks.map((f) => (
                <button
                  key={f.id}
                  type="button"
                  role="radio"
                  aria-checked={os === f.id}
                  onClick={() => { setOs(f.id); if (!name.trim()) setSuggested(suggestName(f.id)) }}
                  className={`flex items-center gap-3 rounded-2xl border p-3 text-left transition ${
                    os === f.id
                      ? 'border-[#0071e3] bg-[color-mix(in_srgb,#0071e3_9%,transparent)] shadow-[0_8px_22px_-14px_rgba(0,113,227,0.7)]'
                      : 'border-[var(--apple-hairline)] hover:border-[color-mix(in_srgb,#0071e3_40%,transparent)]'
                  }`}
                >
                  <span className="text-2xl" aria-hidden>{f.icon && f.icon.length <= 4 ? f.icon : '🐧'}</span>
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-medium text-[var(--text-primary)]">{f.label}</span>
                    <span className="block truncate text-xs text-[var(--text-muted)]">{f.subtitle}</span>
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>

        <div>
          <p className="mb-2 text-xs font-semibold uppercase tracking-[0.12em] text-[var(--text-muted)]">Size</p>
          <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Size">
            {QUICK_SIZES.map((s) => (
              <button
                key={s.id}
                type="button"
                role="radio"
                aria-checked={size === s.id}
                onClick={() => setSize(s.id)}
                className={`rounded-2xl border p-3 text-left transition ${size === s.id ? 'border-[#0071e3] bg-[color-mix(in_srgb,#0071e3_9%,transparent)]' : 'border-[var(--apple-hairline)] hover:border-[color-mix(in_srgb,#0071e3_40%,transparent)]'}`}
              >
                <span className="block text-sm font-medium text-[var(--text-primary)]">{s.label}</span>
                <span className="block text-xs text-[var(--text-muted)]">{s.cores} vCPU · {s.memoryGiB} GiB · {s.diskGiB} GiB</span>
                <span className="mt-0.5 block text-[11px] text-[var(--text-faint)]">{s.detail}</span>
              </button>
            ))}
          </div>
        </div>

        <div>
          <label htmlFor="quick-vm-name" className="mb-2 block text-xs font-semibold uppercase tracking-[0.12em] text-[var(--text-muted)]">Name</label>
          <input id="quick-vm-name" className="input w-full text-sm" value={name} placeholder={suggested} onChange={(e) => setName(e.target.value)} autoComplete="off" />
          <p className="mt-1 text-xs text-[var(--text-muted)]">Leave empty to use <code>{suggested}</code>. Network: {network}.</p>
        </div>

        {needsReadiness && matched ? (
          <VmWizardReadinessBanner loading={readinessLoading} readiness={readiness} templateName={matched.name} templateVersion={matched.version} onReadinessChange={setReadiness} />
        ) : null}

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--apple-hairline)] pt-4">
          <button type="button" className="text-sm text-[var(--link)] hover:underline" onClick={onAdvanced}>More options →</button>
          <div className="flex items-center gap-2">
            <button type="button" className="btn-secondary text-sm" onClick={onClose}>Cancel</button>
            <button type="button" className="btn-primary text-sm" disabled={busy || blocked || (!flavor && picks.length > 0)} onClick={() => void create()}>
              {busy ? 'Creating…' : 'Create VM'}
            </button>
          </div>
        </div>
      </div>
    </GlassModal>
  )
}
